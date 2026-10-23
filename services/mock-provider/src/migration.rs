//! One explicit V2→V3 owner exchange. No predecessor business engine runs here.
use crate::{Error, ReferenceState, Result, require};
use generated_contracts::{
    AccountId32, EntityId, ExecutionOutcome, PayloadHash, UnsignedContractCallV1,
    provider_core::{
        CreationContentV1, CreationMetadataRecordV1, MetadataEntityV1, MetadataReferenceV1,
        MilestoneCore, MilestoneTerms, ProjectCore, ProjectCoreV2, ProposalContentReferenceV1,
        ProposalContentV1, ProposalCore, ProposalMetadataRecordV1, ProposalProvenanceV1,
    },
};
use provider_execution::migration::{
    CoreArchiveV2, CoreArchiveV3, CoreCheckpointV2, CoreCheckpointV3,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Hash exact immutable owner-export bytes; never normalize/reencode first.
#[must_use]
pub fn source_hash(bytes: &[u8]) -> PayloadHash {
    PayloadHash::from_bytes(Sha256::digest(bytes).into())
}
fn migrated(
    generation: generated_contracts::ProviderInstanceId,
    source_archive_hash: PayloadHash,
    project_id: EntityId,
    proposal_id: EntityId,
    content: ProposalContentV1,
    records: &mut Vec<ProposalMetadataRecordV1>,
) -> ProposalContentReferenceV1 {
    let record = ProposalMetadataRecordV1 {
        generation,
        project_id,
        provenance: ProposalProvenanceV1::Migration {
            source_archive_hash,
            proposal_id,
        },
        content,
    };
    let reference = ProposalContentReferenceV1::Migration(source_hash(&record.hash_input()));
    records.push(record);
    reference
}
fn compact_proposals(
    proposals: Vec<generated_contracts::ProposalView>,
    generation: generated_contracts::ProviderInstanceId,
    source: PayloadHash,
    id: EntityId,
    records: &mut Vec<ProposalMetadataRecordV1>,
) -> Result<Vec<ProposalCore>> {
    let mut result = Vec::new();
    for proposal in proposals {
        let definition = generated_contracts::ProposalDefinition {
            title: proposal.title,
            description: proposal.description,
            milestones: proposal
                .milestones
                .iter()
                .map(|m| m.definition.clone())
                .collect(),
        };
        definition.validate()?;
        let content = migrated(
            generation,
            source,
            id,
            proposal.proposal_id,
            ProposalContentV1::Definition(definition),
            records,
        );
        let change_request = proposal.change_request.map(|text| {
            migrated(
                generation,
                source,
                id,
                proposal.proposal_id,
                ProposalContentV1::ChangeRequest(text),
                records,
            )
        });
        let milestones = proposal
            .milestones
            .into_iter()
            .map(|m| MilestoneCore {
                definition: MilestoneTerms::from(&m.definition),
                milestone_id: m.milestone_id,
                status: m.status,
                assignments: m.assignments,
                worker_ratings: m.worker_ratings,
                task_storage: m.task_storage,
                frozen: m.frozen,
                submissions: m.submissions,
            })
            .collect();
        result.push(ProposalCore {
            proposal_id: proposal.proposal_id,
            revision: proposal.revision,
            content,
            status: proposal.status,
            milestones,
            change_request,
            delivery: proposal.delivery,
            compensation_policy: proposal.compensation_policy,
        });
    }
    Ok(result)
}
/// Retain every current fact/history while moving proposal text to its API owner.
pub fn from_predecessor(
    bytes: &[u8],
    root: AccountId32,
) -> Result<(CoreArchiveV3, Vec<ProposalMetadataRecordV1>)> {
    let old: CoreArchiveV2 =
        serde_json::from_slice(bytes).map_err(|_| Error::domain("invalid_migration_archive"))?;
    require(
        old.version == 2 && old.checkpoint.engine_version == 2,
        "invalid_migration_archive",
    )?;
    old.checkpoint.shell().validate(root)?;
    let source = source_hash(bytes);
    let old = old.checkpoint;
    let generation = old.info.provider_instance_id;
    let mut records = Vec::new();
    let mut projects = BTreeMap::new();
    for (id, project) in old.projects {
        let proposals = compact_proposals(project.proposals, generation, source, id, &mut records)?;
        projects.insert(
            id,
            ProjectCore {
                project_id: project.project_id,
                created_at: project.created_at,
                client: project.client,
                coordinator: project.coordinator,
                content: project.content,
                planning: project.planning,
                proposals,
                execution_escrow: project.execution_escrow,
                cancelled: project.cancelled,
                completed: project.completed,
                evaluations: project.evaluations,
                active_dispute_id: project.active_dispute_id,
            },
        );
    }
    let archive = CoreArchiveV3 {
        version: 3,
        checkpoint: CoreCheckpointV3 {
            info: old.info,
            roles: old.roles,
            skills: old.skills,
            skill_metadata: old.skill_metadata,
            skill_requests: old.skill_requests,
            score_policy: old.score_policy,
            compensation_policy: old.compensation_policy,
            workers: old.workers,
            balances: old.balances,
            nonces: old.nonces,
            receipts: old.receipts,
            events: old.events,
            reasons: old.reasons,
            disputes: old.disputes,
            bramp: old.bramp,
            next_task_id: old.next_task_id,
            minted_units: old.minted_units,
            projects,
            engine_version: 3,
        },
    };
    Ok((
        ReferenceState::from_archive(root, archive)?.archive()?,
        records,
    ))
}
fn record<'a>(
    generation: generated_contracts::ProviderInstanceId,
    project_id: EntityId,
    proposal_id: EntityId,
    author: AccountId32,
    reference: &ProposalContentReferenceV1,
    records: &'a BTreeMap<PayloadHash, ProposalMetadataRecordV1>,
) -> Result<&'a ProposalMetadataRecordV1> {
    let record = records
        .get(&reference.hash())
        .ok_or_else(|| Error::domain("metadata_record_missing"))?;
    require(
        record.matches_context(generation, project_id, proposal_id, author, reference)
            && source_hash(&record.hash_input()) == reference.hash(),
        "invalid_metadata_record",
    )?;
    Ok(record)
}
/// Rehydrate only proposal fields; R2a creation refs and every recorded byte stay intact.
pub fn to_predecessor(
    archive: CoreArchiveV3,
    records: &BTreeMap<PayloadHash, ProposalMetadataRecordV1>,
) -> Result<CoreArchiveV2> {
    let saved = ReferenceState::from_archive(archive.checkpoint.info.root_account, archive)?
        .archive()?
        .checkpoint;
    let generation = saved.info.provider_instance_id;
    let mut projects = BTreeMap::new();
    for (id, project) in saved.projects {
        let mut proposals = Vec::new();
        for proposal in project.proposals {
            let selected = record(
                generation,
                id,
                proposal.proposal_id,
                project.coordinator,
                &proposal.content,
                records,
            )?;
            let definition = proposal.definition(selected)?;
            let change = proposal
                .change_request
                .as_ref()
                .map(|reference| {
                    let selected = record(
                        generation,
                        id,
                        proposal.proposal_id,
                        project.client,
                        reference,
                        records,
                    )?;
                    if let ProposalContentV1::ChangeRequest(text) = &selected.content {
                        Ok(text.clone())
                    } else {
                        Err(Error::domain("invalid_metadata_record"))
                    }
                })
                .transpose()?;
            proposals.push(proposal.into_view(definition, change)?);
        }
        projects.insert(
            id,
            ProjectCoreV2 {
                project_id: project.project_id,
                created_at: project.created_at,
                client: project.client,
                coordinator: project.coordinator,
                content: project.content,
                planning: project.planning,
                proposals,
                execution_escrow: project.execution_escrow,
                cancelled: project.cancelled,
                completed: project.completed,
                evaluations: project.evaluations,
                active_dispute_id: project.active_dispute_id,
            },
        );
    }
    Ok(CoreArchiveV2 {
        version: 2,
        checkpoint: CoreCheckpointV2 {
            info: saved.info,
            roles: saved.roles,
            skills: saved.skills,
            skill_metadata: saved.skill_metadata,
            skill_requests: saved.skill_requests,
            score_policy: saved.score_policy,
            compensation_policy: saved.compensation_policy,
            workers: saved.workers,
            balances: saved.balances,
            nonces: saved.nonces,
            receipts: saved.receipts,
            events: saved.events,
            reasons: saved.reasons,
            disputes: saved.disputes,
            bramp: saved.bramp,
            next_task_id: saved.next_task_id,
            minted_units: saved.minted_units,
            projects,
            engine_version: 2,
        },
    })
}
/// Recover only successful legacy operations selected by current core refs.
/// Compact operation preimages still require the API's backup/export.
pub fn recover_legacy_proposals(archive: &CoreArchiveV3) -> Result<Vec<ProposalMetadataRecordV1>> {
    let saved =
        ReferenceState::from_archive(archive.checkpoint.info.root_account, archive.clone())?
            .archive()?
            .checkpoint;
    let mut records = Vec::new();
    for accepted in saved.receipts.values() {
        if accepted.receipt.outcome != ExecutionOutcome::Success {
            continue;
        }
        let call = UnsignedContractCallV1::decode_signable(&accepted.bytes)?;
        let Some(candidate) = ProposalMetadataRecordV1::from_command(
            call.provider_instance_id,
            call.origin,
            call.operation_id,
            &call.command,
        ) else {
            continue;
        };
        let Some(project) = saved.projects.get(&candidate.project_id) else {
            continue;
        };
        let Some(proposal) = accepted
            .receipt
            .created_entity_id
            .and_then(|id| project.proposals.iter().find(|p| p.proposal_id == id))
        else {
            continue;
        };
        let (reference, author) = match &candidate.content {
            ProposalContentV1::Definition(_) => (Some(&proposal.content), project.coordinator),
            ProposalContentV1::ChangeRequest(_) => {
                (proposal.change_request.as_ref(), project.client)
            }
        };
        let Some(ProposalContentReferenceV1::Signed(commitment)) = reference else {
            continue;
        };
        if commitment.operation_id != call.operation_id {
            continue;
        }
        require(
            candidate.matches_context(
                saved.info.provider_instance_id,
                project.project_id,
                proposal.proposal_id,
                author,
                reference.ok_or_else(Error::internal)?,
            ) && source_hash(&candidate.hash_input()) == commitment.hash,
            "invalid_metadata_record",
        )?;
        if matches!(candidate.content, ProposalContentV1::Definition(_)) {
            proposal.definition(&candidate)?;
        }
        records.push(candidate);
    }
    Ok(records)
}

/// Reconstruct successful legacy creation preimages from retained owner history.
/// This is recovery for direct legacy calls; normal API writes stage before signing.
pub fn recover_legacy_creations(archive: &CoreArchiveV3) -> Result<Vec<CreationMetadataRecordV1>> {
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
