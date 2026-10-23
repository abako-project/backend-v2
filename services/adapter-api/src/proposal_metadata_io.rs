//! Owner-only proposal exchange. Core selection comes through HTTP, never `SQLite`.
use crate::{
    creation_metadata_io::{next, reconciled, write},
    state::{App, Error, hash},
};
use generated_contracts::{
    AccountId32, EntityId, PayloadHash, ProviderInstanceId,
    provider_core::{
        CoreSnapshotV3, ProposalContentReferenceV1, ProposalContentV1, ProposalCore,
        ProposalMetadataHeaderV1, ProposalMetadataRecordV1,
    },
};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::BufReader,
    path::PathBuf,
};
struct Selected {
    project: EntityId,
    author: AccountId32,
    proposal: ProposalCore,
    reference: ProposalContentReferenceV1,
    change: bool,
}
impl Selected {
    fn verify(
        &self,
        generation: ProviderInstanceId,
        record: &ProposalMetadataRecordV1,
    ) -> Result<(), Error> {
        if !record.matches_context(
            generation,
            self.project,
            self.proposal.proposal_id,
            self.author,
            &self.reference,
        ) || hash(&record.hash_input()) != self.reference.hash().into_bytes()
        {
            return Err(Error::Dependency);
        }
        if self.change {
            if !matches!(record.content, ProposalContentV1::ChangeRequest(_)) {
                return Err(Error::Dependency);
            }
        } else {
            self.proposal
                .definition(record)
                .map_err(|_| Error::Dependency)?;
        }
        Ok(())
    }
}
fn selected(snapshot: CoreSnapshotV3) -> Result<BTreeMap<PayloadHash, Selected>, Error> {
    let mut all = BTreeMap::new();
    for project in snapshot.projects {
        for proposal in project.proposals {
            let reference = proposal.content.clone();
            let selection = Selected {
                project: project.project_id,
                author: project.coordinator,
                proposal: proposal.clone(),
                reference,
                change: false,
            };
            if all.insert(selection.reference.hash(), selection).is_some() {
                return Err(Error::Dependency);
            }
            if let Some(reference) = proposal.change_request.clone() {
                let selection = Selected {
                    project: project.project_id,
                    author: project.client,
                    proposal,
                    reference,
                    change: true,
                };
                if all.insert(selection.reference.hash(), selection).is_some() {
                    return Err(Error::Dependency);
                }
            }
        }
    }
    Ok(all)
}
pub(crate) async fn import(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let mut expected = tokio::task::spawn_blocking(move || selected(snapshot)).await??;
    let count = expected.len();
    let reader =
        tokio::task::spawn_blocking(move || File::open(path).map(BufReader::new)).await??;
    let (mut reader, header) = next::<ProposalMetadataHeaderV1>(reader).await?;
    let header = header.ok_or(Error::Invalid)?;
    if header.schema_version != 1 || header.generation != generation {
        return Err(Error::Dependency);
    }
    let mut tx = app.db.begin().await?;
    loop {
        let (next_reader, record) = next::<ProposalMetadataRecordV1>(reader).await?;
        reader = next_reader;
        let Some(record) = record else {
            break;
        };
        let fingerprint = tokio::task::spawn_blocking(move || {
            let fingerprint = hash(&record.hash_input());
            (record, fingerprint)
        })
        .await?;
        let (record, fingerprint) = fingerprint;
        let selected = expected
            .remove(&PayloadHash::from_bytes(fingerprint))
            .ok_or(Error::Dependency)?;
        let payload = tokio::task::spawn_blocking(move || {
            selected.verify(generation, &record)?;
            Ok::<_, Error>(serde_json::to_string(&record)?)
        })
        .await??;
        crate::proposal_metadata::insert(&mut tx, generation, fingerprint, &payload).await?;
    }
    for (_, selection) in expected {
        let record = crate::proposal_metadata::record(
            app,
            generation,
            selection.project,
            selection.proposal.proposal_id,
            selection.author,
            &selection.reference,
        )
        .await?;
        tokio::task::spawn_blocking(move || selection.verify(generation, &record)).await??;
    }
    tx.commit().await?;
    Ok(count)
}
pub(crate) async fn export(app: &App, path: PathBuf) -> Result<usize, Error> {
    reconciled(app).await?;
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let selected = tokio::task::spawn_blocking(move || selected(snapshot)).await??;
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
        ProposalMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
    )
    .await?;
    for (_, selection) in selected {
        let record = crate::proposal_metadata::record(
            app,
            generation,
            selection.project,
            selection.proposal.proposal_id,
            selection.author,
            &selection.reference,
        )
        .await?;
        let record = tokio::task::spawn_blocking(move || {
            selection.verify(generation, &record)?;
            Ok::<_, Error>(record)
        })
        .await??;
        file = write(file, record).await?;
    }
    tokio::task::spawn_blocking(move || file.sync_all()).await??;
    Ok(count)
}
