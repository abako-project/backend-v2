//! Owner-only bounded reason exchange. No reason-history route is public.
use crate::{
    creation_metadata_io::{next, reconciled, write},
    state::{App, Error, hash},
};
use generated_contracts::{
    PayloadHash, ProviderInstanceId,
    provider_core::{ReasonCore, ReasonMetadataHeaderV1, ReasonMetadataRecordV1, ReasonPageV1},
};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::BufReader,
    path::PathBuf,
};
async fn selected(
    app: &App,
) -> Result<(ProviderInstanceId, BTreeMap<PayloadHash, (u64, ReasonCore)>), Error> {
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let mut after = 0u64;
    let mut total = None;
    let mut selected = BTreeMap::new();
    loop {
        let page: ReasonPageV1 = app
            .get(
                &app.config.provider_url,
                &format!("/internal/lifecycle-reasons?after={after}&limit=256"),
            )
            .await?;
        if page.generation != generation
            || page.start != after
            || page.reasons.len() > 256
            || page.next
                != after
                    .checked_add(u64::try_from(page.reasons.len()).map_err(|_| Error::Dependency)?)
                    .ok_or(Error::Dependency)?
            || page.next > page.total
            || total.is_some_and(|t| t != page.total)
            || (page.next == after && after != page.total)
        {
            return Err(Error::Dependency);
        }
        total = Some(page.total);
        for (offset, reason) in page.reasons.into_iter().enumerate() {
            let index = after
                .checked_add(u64::try_from(offset).map_err(|_| Error::Dependency)?)
                .ok_or(Error::Dependency)?;
            if selected
                .insert(reason.content.hash(), (index, reason))
                .is_some()
            {
                return Err(Error::Dependency);
            }
        }
        after = page.next;
        if after == page.total {
            break;
        }
    }
    Ok((generation, selected))
}
pub(crate) async fn import(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let (generation, mut selected) = selected(app).await?;
    let count = selected.len();
    let reader =
        tokio::task::spawn_blocking(move || File::open(path).map(BufReader::new)).await??;
    let (mut reader, header) = next::<ReasonMetadataHeaderV1>(reader).await?;
    if !header.is_some_and(|h| h.schema_version == 1 && h.generation == generation) {
        return Err(Error::Dependency);
    }
    let mut tx = app.db.begin().await?;
    loop {
        let (next_reader, record) = next::<ReasonMetadataRecordV1>(reader).await?;
        reader = next_reader;
        let Some(record) = record else {
            break;
        };
        let (record, fingerprint) = tokio::task::spawn_blocking(move || {
            let fingerprint = hash(&record.hash_input());
            (record, fingerprint)
        })
        .await?;
        let (index, core) = selected
            .remove(&PayloadHash::from_bytes(fingerprint))
            .ok_or(Error::Dependency)?;
        let payload = tokio::task::spawn_blocking(move || {
            if !record.matches_context(generation, index, &core) {
                return Err(Error::Dependency);
            }
            Ok::<_, Error>(serde_json::to_string(&record)?)
        })
        .await??;
        crate::reason_metadata::insert(&mut tx, generation, fingerprint, &payload).await?;
    }
    for (_, (index, core)) in selected {
        crate::reason_metadata::record(app, generation, index, core).await?;
    }
    tx.commit().await?;
    Ok(count)
}
pub(crate) async fn export(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let (generation, selected) = selected(app).await?;
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
        ReasonMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
    )
    .await?;
    for (_, (index, core)) in selected {
        file = write(
            file,
            crate::reason_metadata::record(app, generation, index, core).await?,
        )
        .await?;
    }
    tokio::task::spawn_blocking(move || file.sync_all()).await??;
    Ok(count)
}
