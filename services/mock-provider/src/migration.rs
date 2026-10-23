//! One explicit predecessor exchange. No old business engine runs here.
//! The caller uses owner exports and fresh stores, never cross-service tables.
use crate::{Error, ReferenceState, Result, require};
use generated_contracts::{
    ExecutionOutcome, PayloadHash, UnsignedContractCallV1,
    provider_core::{
        CreationContentV1, CreationMetadataRecordV1, MetadataEntityV1, MetadataProvenanceV1,
        MetadataReferenceV1, ProjectCore, WorkerCore,
    },
};
use provider_execution::migration::{ArchiveV1, CheckpointV1, CoreArchiveV2, CoreCheckpointV2};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Hash the exact immutable file bytes; do not reserialize before hashing.
#[must_use]
pub fn source_hash(bytes: &[u8]) -> PayloadHash {
    PayloadHash::from_bytes(Sha256::digest(bytes).into())
}

/// Retain all facts/history while moving the two descriptive roots offchain.
/// The returned records have explicit migration provenance, including entities
/// whose original creation operation is absent from the retained history.
pub fn from_predecessor(
    bytes: &[u8],
    root: generated_contracts::AccountId32,
) -> Result<(CoreArchiveV2, Vec<CreationMetadataRecordV1>)> {
    let old: ArchiveV1 =
        serde_json::from_slice(bytes).map_err(|_| Error::domain("invalid_migration_archive"))?;
    require(
        old.version == 1 && old.checkpoint.engine_version == 1,
        "invalid_migration_archive",
    )?;
    old.checkpoint.shell().validate(root)?;
    let source = source_hash(bytes);
    let old = old.checkpoint;
    let generation = old.info.provider_instance_id;
    let mut records = Vec::new();
    let mut workers = BTreeMap::new();
    for (account, worker) in old.workers {
        let record = CreationMetadataRecordV1 {
            generation,
            provenance: MetadataProvenanceV1::Migration {
                source_archive_hash: source,
                entity: MetadataEntityV1::Worker(account),
            },
            content: CreationContentV1::WorkerName {
                display_name: worker.display_name,
            },
        };
        let content = MetadataReferenceV1::Migration(source_hash(&record.hash_input()));
        records.push(record);
        workers.insert(
            account,
            WorkerCore {
                account: worker.account,
                content,
                qualifications: worker.qualifications,
                mode: worker.mode,
                coordinator_eligible: worker.coordinator_eligible,
                calendar: worker.calendar,
                worker_score: worker.worker_score,
                coordinator_score: worker.coordinator_score,
            },
        );
    }
    let mut projects = BTreeMap::new();
    for (id, project) in old.projects {
        let record = CreationMetadataRecordV1 {
            generation,
            provenance: MetadataProvenanceV1::Migration {
                source_archive_hash: source,
                entity: MetadataEntityV1::Project(id),
            },
            content: CreationContentV1::ProjectText {
                title: project.title,
                description: project.description,
            },
        };
        let content = MetadataReferenceV1::Migration(source_hash(&record.hash_input()));
        records.push(record);
        projects.insert(
            id,
            ProjectCore {
                project_id: project.project_id,
                created_at: project.created_at,
                client: project.client,
                coordinator: project.coordinator,
                content,
                planning: project.planning,
                proposals: project.proposals,
                execution_escrow: project.execution_escrow,
                cancelled: project.cancelled,
                completed: project.completed,
                evaluations: project.evaluations,
                active_dispute_id: project.active_dispute_id,
            },
        );
    }
    let archive = CoreArchiveV2 {
        version: 2,
        checkpoint: CoreCheckpointV2 {
            info: old.info,
            roles: old.roles,
            skills: old.skills,
            skill_metadata: old.skill_metadata,
            skill_requests: old.skill_requests,
            score_policy: old.score_policy,
            compensation_policy: old.compensation_policy,
            workers,
            projects,
            balances: old.balances,
            nonces: old.nonces,
            receipts: old.receipts,
            events: old.events,
            reasons: old.reasons,
            disputes: old.disputes,
            bramp: old.bramp,
            next_task_id: old.next_task_id,
            minted_units: old.minted_units,
            engine_version: 2,
        },
    };
    let archive = ReferenceState::from_archive(root, archive)?.archive()?;
    Ok((archive, records))
}

fn content(
    generation: generated_contracts::ProviderInstanceId,
    entity: &MetadataEntityV1,
    owner: generated_contracts::AccountId32,
    reference: &MetadataReferenceV1,
    records: &BTreeMap<PayloadHash, CreationMetadataRecordV1>,
) -> Result<CreationContentV1> {
    let record = records
        .get(&reference.hash())
        .ok_or_else(|| Error::domain("metadata_record_missing"))?;
    require(
        record.matches_context(generation, entity, owner, reference)
            && source_hash(&record.hash_input()) == reference.hash(),
        "invalid_metadata_record",
    )?;
    Ok(record.content.clone())
}

/// Rehydrate the immediately preceding cohort with *current* facts and exact
/// retained calls, including compact command bytes recognized by its bridge.
pub fn to_predecessor(
    archive: CoreArchiveV2,
    records: &BTreeMap<PayloadHash, CreationMetadataRecordV1>,
) -> Result<ArchiveV1> {
    let root = archive.checkpoint.info.root_account;
    let saved = ReferenceState::from_archive(root, archive)?
        .archive()?
        .checkpoint;
    let generation = saved.info.provider_instance_id;
    let mut workers = BTreeMap::new();
    for (account, worker) in saved.workers {
        let CreationContentV1::WorkerName { display_name } = content(
            generation,
            &MetadataEntityV1::Worker(account),
            account,
            &worker.content,
            records,
        )?
        else {
            return Err(Error::domain("invalid_metadata_record"));
        };
        workers.insert(account, worker.into_view(display_name));
    }
    let mut projects = BTreeMap::new();
    for (id, project) in saved.projects {
        let CreationContentV1::ProjectText { title, description } = content(
            generation,
            &MetadataEntityV1::Project(id),
            project.client,
            &project.content,
            records,
        )?
        else {
            return Err(Error::domain("invalid_metadata_record"));
        };
        projects.insert(id, project.into_view(title, description));
    }
    Ok(ArchiveV1 {
        version: 1,
        checkpoint: CheckpointV1 {
            info: saved.info,
            roles: saved.roles,
            skills: saved.skills,
            skill_metadata: saved.skill_metadata,
            skill_requests: saved.skill_requests,
            score_policy: saved.score_policy,
            compensation_policy: saved.compensation_policy,
            workers,
            projects,
            balances: saved.balances,
            nonces: saved.nonces,
            receipts: saved.receipts,
            events: saved.events,
            reasons: saved.reasons,
            disputes: saved.disputes,
            bramp: saved.bramp,
            next_task_id: saved.next_task_id,
            minted_units: saved.minted_units,
            engine_version: 1,
        },
    })
}

/// Reconstruct successful legacy creation preimages from retained owner history.
/// This is recovery for direct legacy calls; normal API writes stage before signing.
pub fn recover_legacy_creations(archive: &CoreArchiveV2) -> Result<Vec<CreationMetadataRecordV1>> {
    let validated =
        ReferenceState::from_archive(archive.checkpoint.info.root_account, archive.clone())?
            .archive()?;
    let saved = &validated.checkpoint;
    let mut records = Vec::new();
    for accepted in saved.receipts.values() {
        if accepted.receipt.outcome != ExecutionOutcome::Success {
            continue;
        }
        let call = UnsignedContractCallV1::decode_signable(&accepted.bytes)?;
        let Some(record) = CreationMetadataRecordV1::from_creation_command(
            call.provider_instance_id,
            call.origin,
            call.operation_id,
            &call.command,
        ) else {
            continue;
        };
        let selected = match &record.content {
            CreationContentV1::WorkerName { .. } => saved
                .workers
                .get(&call.origin)
                .filter(|w| accepted.receipt.created_entity_id == Some(w.calendar.calendar_id))
                .map(|w| (MetadataEntityV1::Worker(w.account), w.account, &w.content)),
            CreationContentV1::ProjectText { .. } => accepted
                .receipt
                .created_entity_id
                .and_then(|id| saved.projects.get(&id))
                .map(|p| {
                    (
                        MetadataEntityV1::Project(p.project_id),
                        p.client,
                        &p.content,
                    )
                }),
        };
        let Some((entity, owner, reference @ MetadataReferenceV1::Creation(commitment))) = selected
        else {
            continue;
        };
        if commitment.operation_id != call.operation_id {
            continue;
        }
        require(
            record.matches_context(saved.info.provider_instance_id, &entity, owner, reference)
                && source_hash(&record.hash_input()) == reference.hash(),
            "invalid_metadata_record",
        )?;
        records.push(record);
    }
    Ok(records)
}
