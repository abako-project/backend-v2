//! Owner-only catalog exchange. Shared label/origin/request refs are one file line,
//! checked against every selected context before the single import commit.
use crate::{
    catalog_metadata as metadata,
    creation_metadata_io::{next, reconciled, write},
    state::{App, Error, hash},
};
use generated_contracts::{
    CatalogKind, PayloadHash,
    provider_core::{
        CatalogContentV1, CatalogMetadataHeaderV1, CatalogMetadataRecordV1, CatalogReferenceV1,
        CoreSnapshotV5, SkillRequestCore,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::BufReader,
    path::PathBuf,
};
struct Selection {
    snapshot: CoreSnapshotV5,
    requests: Vec<SkillRequestCore>,
    references: BTreeMap<PayloadHash, CatalogReferenceV1>,
}
async fn selection(app: &App) -> Result<Selection, Error> {
    let snapshot = app.snapshot().await?;
    let requests = metadata::requests(app, snapshot.info.root_account).await?;
    let mut references = BTreeMap::new();
    for reference in snapshot
        .catalog
        .roles
        .iter()
        .chain(&snapshot.catalog.skills)
        .map(|e| &e.content)
        .chain(snapshot.catalog.skill_roles.iter().map(|e| &e.origin))
        .chain(requests.iter().map(|r| &r.content))
    {
        if let Some(previous) = references.insert(reference.hash(), reference.clone())
            && previous != *reference
        {
            return Err(Error::Dependency);
        }
    }
    Ok(Selection {
        snapshot,
        requests,
        references,
    })
}
impl Selection {
    fn verify(
        &self,
        records: &BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
    ) -> Result<(), Error> {
        let generation = self.snapshot.info.provider_instance_id;
        let root = self.snapshot.info.root_account;
        let record = |reference: &CatalogReferenceV1| -> Result<&CatalogMetadataRecordV1, Error> {
            let record = records.get(&reference.hash()).ok_or(Error::Dependency)?;
            if record.generation != generation
                || hash(&record.hash_input()) != reference.hash().into_bytes()
            {
                return Err(Error::Dependency);
            }
            Ok(record)
        };
        let mut names = BTreeSet::new();
        for (kind, entries) in [
            (CatalogKind::Role, &self.snapshot.catalog.roles),
            (CatalogKind::Skill, &self.snapshot.catalog.skills),
        ] {
            for entry in entries {
                let name = record(&entry.content)?
                    .entry_name(
                        generation,
                        kind,
                        entry.id,
                        root,
                        &entry.content,
                        &self.requests,
                    )
                    .map_err(|_| Error::Dependency)?;
                if kind == CatalogKind::Skill
                    && !names.insert(CatalogMetadataRecordV1::normalized(name))
                {
                    return Err(Error::Dependency);
                }
            }
        }
        for origin in &self.snapshot.catalog.skill_roles {
            let document = record(&origin.origin)?;
            document
                .category(
                    generation,
                    origin.skill_id,
                    root,
                    &origin.origin,
                    &self.requests,
                )
                .map_err(|_| Error::Dependency)?;
            if !matches!(document.content, CatalogContentV1::Entry { .. })
                && !self.requests.iter().any(|request| {
                    request.status == generated_contracts::SkillRequestStatus::Approved
                        && request.skill_id == Some(origin.skill_id)
                        && request.content == origin.origin
                        && request.role_ids == origin.role_ids
                })
            {
                return Err(Error::Dependency);
            }
        }
        for request in &self.requests {
            record(&request.content)?
                .request_name(generation, request)
                .map_err(|_| Error::Dependency)?;
        }
        Ok(())
    }
}
pub(crate) async fn import(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let selection = selection(app).await?;
    let generation = selection.snapshot.info.provider_instance_id;
    let mut expected = selection.references.clone();
    let count = expected.len();
    let reader =
        tokio::task::spawn_blocking(move || File::open(path).map(BufReader::new)).await??;
    let (mut reader, header) = next::<CatalogMetadataHeaderV1>(reader).await?;
    if !header.is_some_and(|h| h.schema_version == 1 && h.generation == generation) {
        return Err(Error::Dependency);
    }
    let mut records = BTreeMap::new();
    let mut tx = app.db.begin().await?;
    loop {
        let (next_reader, record) = next::<CatalogMetadataRecordV1>(reader).await?;
        reader = next_reader;
        let Some(record) = record else {
            break;
        };
        let (record, fingerprint, payload) = tokio::task::spawn_blocking(move || {
            Ok::<_, Error>((
                record.clone(),
                hash(&record.hash_input()),
                serde_json::to_string(&record)?,
            ))
        })
        .await??;
        let key = PayloadHash::from_bytes(fingerprint);
        expected.remove(&key).ok_or(Error::Dependency)?;
        metadata::insert(&mut tx, generation, fingerprint, &payload).await?;
        records.insert(key, record);
    }
    // A recovery file may omit only records already verified in the owner's DB.
    for (key, _) in expected {
        let payload:String=sqlx::query_scalar("SELECT payload FROM catalog_documents WHERE provider_instance_id=$1 AND content_hash=$2").bind(generation.to_string()).bind(key.as_bytes().as_slice()).fetch_optional(&mut *tx).await?.ok_or(Error::Dependency)?;
        let record = tokio::task::spawn_blocking(move || {
            serde_json::from_str(&payload).map_err(|_| Error::Dependency)
        })
        .await??;
        records.insert(key, record);
    }
    tokio::task::spawn_blocking(move || selection.verify(&records)).await??;
    tx.commit().await?;
    Ok(count)
}
pub(crate) async fn export(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let selection = selection(app).await?;
    let generation = selection.snapshot.info.provider_instance_id;
    metadata::provision_seeds(app, generation, &selection.snapshot.catalog).await?;
    let mut records = BTreeMap::new();
    for (key, reference) in &selection.references {
        records.insert(*key, metadata::record(app, generation, reference).await?);
    }
    let count = records.len();
    let records = tokio::task::spawn_blocking(move || {
        selection.verify(&records)?;
        Ok::<_, Error>(records)
    })
    .await??;
    let mut file = tokio::task::spawn_blocking(move || {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    })
    .await??;
    file = write(
        file,
        CatalogMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
    )
    .await?;
    for (_, record) in records {
        file = write(file, record).await?;
    }
    tokio::task::spawn_blocking(move || file.sync_all()).await??;
    Ok(count)
}
