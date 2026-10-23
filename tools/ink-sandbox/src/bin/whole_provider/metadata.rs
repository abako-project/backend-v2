//! Offline owner-file conversion only. The current binary has one core engine.
use super::{Result, load_archive, write_archive};
use generated_contracts::{
    AccountId32, PayloadHash,
    provider_core::{CreationMetadataHeaderV1, ProposalMetadataHeaderV1, ProposalMetadataRecordV1},
};
use ink_v5_compatibility::worker::MAX_SHELL;
use mock_provider::migration;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};

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
fn write_metadata(
    path: &Path,
    generation: generated_contracts::ProviderInstanceId,
    records: &[ProposalMetadataRecordV1],
) -> Result<()> {
    let mut file = output_file(path)?;
    serde_json::to_writer(
        &mut file,
        &ProposalMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
    )?;
    file.write_all(b"\n")?;
    for record in records {
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
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
fn read_metadata(
    path: &Path,
    generation: generated_contracts::ProviderInstanceId,
) -> Result<BTreeMap<PayloadHash, ProposalMetadataRecordV1>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut total = 0;
    let header: ProposalMetadataHeaderV1 =
        serde_json::from_slice(&line(&mut reader, &mut total)?.ok_or("metadata header missing")?)
            .map_err(|_| "invalid metadata header")?;
    if header.schema_version != 1 || header.generation != generation {
        return Err("metadata generation/version mismatch".into());
    }
    let mut records = BTreeMap::new();
    while let Some(bytes) = line(&mut reader, &mut total)? {
        let record: ProposalMetadataRecordV1 =
            serde_json::from_slice(&bytes).map_err(|_| "invalid metadata record")?;
        if record.generation != generation {
            return Err("metadata generation mismatch".into());
        }
        if records
            .insert(migration::source_hash(&record.hash_input()), record)
            .is_some()
        {
            return Err("duplicate metadata record".into());
        }
    }
    Ok(records)
}

pub(super) fn run(args: &[String], root: AccountId32) -> Result<()> {
    match args {
        [command, source, core_output, metadata_output] if command == "upgrade" => {
            let mut bytes = Vec::new();
            File::open(source)?.take(MAX_SHELL as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > MAX_SHELL { return Err("archive exceeds limit".into()); }
            let (core, records) = migration::from_predecessor(&bytes, root)?;
            write_archive(Path::new(core_output), &core)?;
            write_metadata(Path::new(metadata_output), core.checkpoint.info.provider_instance_id, &records)?;
        }
        [command, source, metadata, output] if command == "downgrade" => {
            let core = load_archive(Path::new(source))?;
            if core.checkpoint.info.root_account != root { return Err("configured root differs from retained root".into()); }
            let records = read_metadata(Path::new(metadata), core.checkpoint.info.provider_instance_id)?;
            if records.len() != core.checkpoint.projects.values().flat_map(|p| &p.proposals).try_fold(0usize, |sum,p|sum.checked_add(1 + usize::from(p.change_request.is_some())).ok_or("entity count overflow"))? {
                return Err("metadata cohort is incomplete or contains unused records".into());
            }
            let old = migration::to_predecessor(core, &records)?;
            write_archive(Path::new(output), &old)?;
        }
        [command, source, output] if command == "recover-creation-metadata" => {
            let core = load_archive(Path::new(source))?;
            if core.checkpoint.info.root_account != root { return Err("configured root differs from retained root".into()); }
            let records = migration::recover_legacy_creations(&core)?;
            let mut file=output_file(Path::new(output))?;
            serde_json::to_writer(&mut file,&CreationMetadataHeaderV1{schema_version:1,generation:core.checkpoint.info.provider_instance_id})?;
            file.write_all(b"\n")?;
            for record in &records { serde_json::to_writer(&mut file,record)?;file.write_all(b"\n")?; }
            file.sync_all()?;
        }
        [command, source, output] if command == "recover-proposal-metadata" => {
            let core=load_archive(Path::new(source))?;
            if core.checkpoint.info.root_account != root {return Err("configured root differs from retained root".into());}
            let records=migration::recover_legacy_proposals(&core)?;
            write_metadata(Path::new(output),core.checkpoint.info.provider_instance_id,&records)?;
        }
        _ => return Err("usage: whole-provider upgrade V2 V3 PROPOSALS | downgrade V3 PROPOSALS V2 | recover-creation-metadata CORE METADATA | recover-proposal-metadata CORE PROPOSALS".into()),
    }
    Ok(())
}
