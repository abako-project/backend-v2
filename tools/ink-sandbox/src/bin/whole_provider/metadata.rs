//! Explicit historical owner exchange and current metadata recovery. One business engine.
use super::{Result, write_archive};
use generated_contracts::{
    AccountId32, PayloadHash, ProviderInstanceId,
    provider_core::{
        CatalogMetadataHeaderV1, CatalogMetadataRecordV1, CreationMetadataHeaderV1,
        ProposalMetadataHeaderV1, ReasonMetadataHeaderV1, ReasonMetadataRecordV1,
    },
};
use ink_v5_compatibility::worker::MAX_SHELL;
use mock_provider::migration;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};
fn read_archive(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_SHELL as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SHELL {
        return Err("archive exceeds limit".into());
    }
    Ok(bytes)
}
// Historical catalog downgrade decodes V4 only. Recovery supports explicitly
// versioned V4 and V5 inputs, without inventing task content for a V5 archive.
fn recovery_archive(
    path: &Path,
    root: AccountId32,
) -> Result<provider_execution::migration::CoreArchiveV5> {
    #[derive(serde::Deserialize)]
    struct Version {
        version: u32,
    }
    let bytes = read_archive(path)?;
    match serde_json::from_slice::<Version>(&bytes)?.version {
        4 => Ok(migration::compact_tasks(root, serde_json::from_slice(&bytes)?)?.0),
        5 => Ok(serde_json::from_slice(&bytes)?),
        _ => Err("unsupported recovery archive version".into()),
    }
}
fn output_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
fn write_metadata<H: Serialize, T: Serialize>(path: &Path, header: H, records: &[T]) -> Result<()> {
    let mut file = output_file(path)?;
    let mut total = 0usize;
    write_line(&mut file, &header, &mut total)?;
    for record in records {
        write_line(&mut file, record, &mut total)?;
    }
    file.sync_all()?;
    Ok(())
}
fn write_line(file: &mut File, value: &impl Serialize, total: &mut usize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let count = bytes.len().checked_add(1).ok_or("metadata exceeds limit")?;
    *total = total.checked_add(count).ok_or("metadata exceeds limit")?;
    if count > 33_554_432 || *total > MAX_SHELL {
        return Err("metadata exceeds limit".into());
    }
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}
fn line(reader: &mut BufReader<File>, total: &mut usize) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let count = reader
        .by_ref()
        .take(33_554_433)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    *total = total.checked_add(count).ok_or("metadata exceeds limit")?;
    if count > 33_554_432 || *total > MAX_SHELL {
        return Err("metadata exceeds limit".into());
    }
    Ok(Some(bytes))
}
fn read_metadata<H: DeserializeOwned, T: DeserializeOwned>(
    path: &Path,
    valid_header: impl FnOnce(H) -> bool,
    key: impl Fn(&T) -> Option<PayloadHash>,
) -> Result<BTreeMap<PayloadHash, T>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut total = 0;
    let header =
        serde_json::from_slice(&line(&mut reader, &mut total)?.ok_or("metadata header missing")?)?;
    if !valid_header(header) {
        return Err("metadata generation/version mismatch".into());
    }
    let mut records = BTreeMap::new();
    while let Some(bytes) = line(&mut reader, &mut total)? {
        let record: T = serde_json::from_slice(&bytes)?;
        let key = key(&record).ok_or("metadata generation mismatch")?;
        if records.insert(key, record).is_some() {
            return Err("duplicate metadata record".into());
        }
    }
    Ok(records)
}
fn catalog_header(generation: ProviderInstanceId) -> CatalogMetadataHeaderV1 {
    CatalogMetadataHeaderV1 {
        schema_version: 1,
        generation,
    }
}
fn reason_header(generation: ProviderInstanceId) -> ReasonMetadataHeaderV1 {
    ReasonMetadataHeaderV1 {
        schema_version: 1,
        generation,
    }
}
pub(super) fn run(args: &[String], root: AccountId32) -> Result<()> {
    match args {
        [command,source,core_output,catalog_output,reason_output] if command=="upgrade"=>{
            let mut bytes=Vec::new();File::open(source)?.take(MAX_SHELL as u64+1).read_to_end(&mut bytes)?;
            if bytes.len()>MAX_SHELL{return Err("archive exceeds limit".into());}
            let (core,catalog,reasons)=migration::from_predecessor(&bytes,root)?;let generation=core.checkpoint.info.provider_instance_id;
            write_archive(Path::new(core_output),&core)?;
            write_metadata(Path::new(catalog_output),catalog_header(generation),&catalog)?;
            write_metadata(Path::new(reason_output),reason_header(generation),&reasons)?;
        }
        [command,source,catalog_path,reason_path,output] if command=="downgrade"=>{
            let core:provider_execution::migration::CoreArchiveV4=serde_json::from_slice(&read_archive(Path::new(source))?)?;if core.checkpoint.info.root_account!=root{return Err("configured root differs from retained root".into());}
            let generation=core.checkpoint.info.provider_instance_id;
            let catalog=read_metadata(Path::new(catalog_path),|h:CatalogMetadataHeaderV1|h.schema_version==1&&h.generation==generation,|r:&CatalogMetadataRecordV1|(r.generation==generation).then(||migration::source_hash(&r.hash_input())))?;
            let reasons=read_metadata(Path::new(reason_path),|h:ReasonMetadataHeaderV1|h.schema_version==1&&h.generation==generation,|r:&ReasonMetadataRecordV1|(r.generation==generation).then(||migration::source_hash(&r.hash_input())))?;
            let saved=&core.checkpoint;
            let expected:BTreeSet<_>=saved.roles.values().chain(saved.skills.values()).map(|e|e.content.hash()).chain(saved.skill_metadata.values().map(|s|s.origin.hash())).chain(saved.skill_requests.values().map(|r|r.content.hash())).collect();
            let expected_reasons:BTreeSet<_>=saved.reasons.iter().map(|r|r.content.hash()).collect();
            if expected!=catalog.keys().copied().collect() || expected_reasons!=reasons.keys().copied().collect(){return Err("metadata cohort is incomplete or contains unused records".into());}
            write_archive(Path::new(output),&migration::to_predecessor(core,&catalog,&reasons)?)?;
        }
        [command,source,output] if matches!(command.as_str(),"recover-creation-metadata"|"recover-proposal-metadata"|"recover-catalog-metadata"|"recover-reason-metadata")=>{
            let core=recovery_archive(Path::new(source),root)?;if core.checkpoint.info.root_account!=root{return Err("configured root differs from retained root".into());}
            let generation=core.checkpoint.info.provider_instance_id;let output=Path::new(output);
            match command.as_str(){
                "recover-creation-metadata"=>write_metadata(output,CreationMetadataHeaderV1{schema_version:1,generation},&migration::recover_current_creations(&core)?)?,
                "recover-proposal-metadata"=>write_metadata(output,ProposalMetadataHeaderV1{schema_version:1,generation},&migration::recover_current_proposals(&core)?)?,
                "recover-catalog-metadata"=>write_metadata(output,catalog_header(generation),&migration::recover_current_catalog(&core)?)?,
                _=>write_metadata(output,reason_header(generation),&migration::recover_current_reasons(&core)?)?,
            }
        }
        _=>return Err("usage: whole-provider upgrade V3 V4 CATALOG REASONS | downgrade V4 CATALOG REASONS V3 | recover-{creation,proposal,catalog,reason}-metadata CORE OUTPUT".into()),
    }
    Ok(())
}
