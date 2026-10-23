//! Owner-only NDJSON exchange for the API's two immutable creation contexts.
//! No provider/wallet database is opened here. Core facts come through HTTP.
use crate::state::{App, Error, hash};
use generated_contracts::{
    AccountId32, PayloadHash,
    provider_core::{
        CoreSnapshotV3, CreationMetadataHeaderV1, CreationMetadataRecordV1, MetadataEntityV1,
        MetadataReferenceV1,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
};

// Same maximum as the existing metadata-table payload, not a new public limit.
const MAX_LINE: u64 = 33_554_432;
type Selected = BTreeMap<PayloadHash, (MetadataEntityV1, AccountId32, MetadataReferenceV1)>;
pub(crate) async fn reconciled(app: &App) -> Result<(), Error> {
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE status NOT IN ('Finalized','Rejected','Expired') OR (status='Expired' AND possibly_submitted))")
        .fetch_one(&app.db).await?;
    if pending {
        return Err(Error::Conflict("cutover_operations_pending"));
    }
    Ok(())
}
fn selected(snapshot: CoreSnapshotV3) -> Result<Selected, Error> {
    let mut selected = BTreeMap::new();
    for worker in snapshot.workers {
        let key = worker.content.hash();
        if selected
            .insert(
                key,
                (
                    MetadataEntityV1::Worker(worker.account),
                    worker.account,
                    worker.content,
                ),
            )
            .is_some()
        {
            return Err(Error::Dependency);
        }
    }
    for project in snapshot.projects {
        let key = project.content.hash();
        if selected
            .insert(
                key,
                (
                    MetadataEntityV1::Project(project.project_id),
                    project.client,
                    project.content,
                ),
            )
            .is_some()
        {
            return Err(Error::Dependency);
        }
    }
    Ok(selected)
}
pub(crate) async fn next<T: DeserializeOwned + Send + 'static>(
    mut reader: BufReader<File>,
) -> Result<(BufReader<File>, Option<T>), Error> {
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        let count = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            return Ok((reader, None));
        }
        if u64::try_from(count).map_err(|_| Error::Invalid)? > MAX_LINE {
            return Err(Error::Invalid);
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        Ok((reader, Some(value)))
    })
    .await?
}
pub(crate) async fn write<T: Serialize + Send + 'static>(
    mut file: File,
    value: T,
) -> Result<File, Error> {
    tokio::task::spawn_blocking(move || {
        let bytes = serde_json::to_vec(&value)?;
        if u64::try_from(bytes.len()).map_err(|_| Error::Invalid)? + 1 > MAX_LINE {
            return Err(Error::Invalid);
        }
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        Ok(file)
    })
    .await?
}

/// Validate a complete metadata cohort against the prepared target core before
/// committing any row. Missing/duplicate/conflicting records roll back together.
pub(crate) async fn import(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let mut expected = selected(snapshot)?;
    let count = expected.len();
    let reader =
        tokio::task::spawn_blocking(move || File::open(path).map(BufReader::new)).await??;
    let (mut reader, header) = next::<CreationMetadataHeaderV1>(reader).await?;
    let header = header.ok_or(Error::Invalid)?;
    if header.schema_version != 1 || header.generation != generation {
        return Err(Error::Dependency);
    }
    let mut tx = app.db.begin().await?;
    loop {
        let (next_reader, record) = next::<CreationMetadataRecordV1>(reader).await?;
        reader = next_reader;
        let Some(record) = record else {
            break;
        };
        let (record, fingerprint, payload) = tokio::task::spawn_blocking(move || {
            let fingerprint = hash(&record.hash_input());
            let payload = serde_json::to_string(&record)?;
            Ok::<_, Error>((record, fingerprint, payload))
        })
        .await??;
        let (entity, owner, reference) = expected
            .remove(&PayloadHash::from_bytes(fingerprint))
            .ok_or(Error::Dependency)?;
        if !record.matches_context(generation, &entity, owner, &reference) {
            return Err(Error::Dependency);
        }
        crate::creation_metadata::insert(
            &mut tx,
            generation,
            matches!(entity, MetadataEntityV1::Worker(_)),
            fingerprint,
            &payload,
        )
        .await?;
    }
    // Recovery files may contain only missing legacy creations. Every other
    // selected reference must already have valid API-owned content; none is skipped.
    for (_, (entity, owner, reference)) in expected {
        crate::creation_metadata::record(app, generation, entity, owner, &reference).await?;
    }
    tx.commit().await?;
    Ok(count)
}

/// Export only content selected by current core refs. Pending/unused records
/// remain in the normal API backup and are not misrepresented as confirmed.
pub(crate) async fn export(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let selected = selected(snapshot)?;
    let count = selected.len();
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
        CreationMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
    )
    .await?;
    for (_, (entity, owner, reference)) in selected {
        let record =
            crate::creation_metadata::record(app, generation, entity, owner, &reference).await?;
        file = write(file, record).await?;
    }
    tokio::task::spawn_blocking(move || file.sync_all()).await??;
    Ok(count)
}
