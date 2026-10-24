//! One explicit V3↔V4 owner exchange. No predecessor business engine runs here.
use crate::{Error, ReferenceState, Result, require};
use generated_contracts::{
    AccountId32, CatalogEntry, CatalogKind, ExecutionOutcome, PayloadHash, SkillRequestView,
    UnsignedContractCallV1,
    provider_core::{
        CatalogContentV1, CatalogEntryCore, CatalogMetadataRecordV1, CatalogProvenanceV1,
        CatalogReferenceV1, CreationContentV1, CreationMetadataRecordV1, MetadataEntityV1,
        MetadataReferenceV1, ProposalContentReferenceV1, ProposalContentV1,
        ProposalMetadataRecordV1, ReasonCore, ReasonMetadataRecordV1, ReasonProvenanceV1,
        ReasonReferenceV1, SkillMetadataCore, SkillRequestCore,
    },
};
use provider_execution::migration::{
    CoreArchiveV3, CoreArchiveV4, CoreCheckpointV3, CoreCheckpointV4,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Hash exact owner export bytes; never reencode/normalize before hashing.
#[must_use]
pub fn source_hash(bytes: &[u8]) -> PayloadHash {
    PayloadHash::from_bytes(Sha256::digest(bytes).into())
}
fn catalog_record(
    generation: generated_contracts::ProviderInstanceId,
    source_archive_hash: PayloadHash,
    content: CatalogContentV1,
    records: &mut Vec<CatalogMetadataRecordV1>,
) -> CatalogReferenceV1 {
    let record = CatalogMetadataRecordV1 {
        generation,
        provenance: CatalogProvenanceV1::Migration {
            source_archive_hash,
        },
        content,
    };
    let reference = CatalogReferenceV1::Migration(source_hash(&record.hash_input()));
    records.push(record);
    reference
}
fn check_legacy_labels(old: &CoreCheckpointV3) -> Result<()> {
    require(
        old.roles
            .get(&1)
            .is_some_and(|r| r.name == "coordinator" && r.fixed),
        "invalid_catalog_state",
    )?;
    let mut names = BTreeSet::new();
    for (id, entry) in &old.roles {
        require(
            *id == entry.id && *id != 0 && entry.fixed == (*id == 1),
            "invalid_catalog_state",
        )?;
        generated_contracts::ProviderCommand::UpsertCatalogEntry(
            generated_contracts::UpsertCatalogEntryRequest {
                kind: CatalogKind::Role,
                id: *id,
                name: entry.name.clone(),
            },
        )
        .validate()?;
    }
    for (id, entry) in &old.skills {
        require(
            *id == entry.id
                && *id != 0
                && !entry.fixed
                && names.insert(CatalogMetadataRecordV1::normalized(&entry.name)),
            "invalid_catalog_state",
        )?;
        generated_contracts::ProviderCommand::UpsertCatalogEntry(
            generated_contracts::UpsertCatalogEntryRequest {
                kind: CatalogKind::Skill,
                id: *id,
                name: entry.name.clone(),
            },
        )
        .validate()?;
    }
    for request in old.skill_requests.values() {
        generated_contracts::ProviderCommand::CreateSkillRequest(
            generated_contracts::CreateSkillRequest {
                name: request.name.clone(),
                role_ids: request.role_ids.clone(),
            },
        )
        .validate()?;
    }
    Ok(())
}
/// Preserve every enforceable fact/history while moving descriptive contexts.
#[allow(
    clippy::too_many_lines,
    reason = "Keep the explicit one-version field transfer and document selection auditable together."
)]
pub fn from_predecessor(
    bytes: &[u8],
    root: AccountId32,
) -> Result<(
    CoreArchiveV4,
    Vec<CatalogMetadataRecordV1>,
    Vec<ReasonMetadataRecordV1>,
)> {
    let archive: CoreArchiveV3 =
        serde_json::from_slice(bytes).map_err(|_| Error::domain("invalid_migration_archive"))?;
    require(
        archive.version == 3 && archive.checkpoint.engine_version == 3,
        "invalid_migration_archive",
    )?;
    archive.checkpoint.shell().validate(root)?;
    check_legacy_labels(&archive.checkpoint)?;
    let source = source_hash(bytes);
    let old = archive.checkpoint;
    let generation = old.info.provider_instance_id;
    let mut records = Vec::new();
    let mut roles = BTreeMap::new();
    let mut skills = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    let mut requests = BTreeMap::new();
    for (id, entry) in &old.roles {
        let content = catalog_record(
            generation,
            source,
            CatalogContentV1::Entry {
                kind: CatalogKind::Role,
                id: *id,
                name: entry.name.clone(),
                category: String::new(),
            },
            &mut records,
        );
        roles.insert(
            *id,
            CatalogEntryCore {
                id: *id,
                fixed: entry.fixed,
                content,
            },
        );
    }
    for (id, entry) in &old.skills {
        let previous = old
            .skill_metadata
            .get(id)
            .ok_or_else(|| Error::domain("invalid_catalog_state"))?;
        let content = catalog_record(
            generation,
            source,
            CatalogContentV1::Entry {
                kind: CatalogKind::Skill,
                id: *id,
                name: entry.name.clone(),
                category: previous.category.clone(),
            },
            &mut records,
        );
        skills.insert(
            *id,
            CatalogEntryCore {
                id: *id,
                fixed: entry.fixed,
                content: content.clone(),
            },
        );
        metadata.insert(
            *id,
            SkillMetadataCore {
                role_ids: previous.role_ids.clone(),
                origin: content,
            },
        );
    }
    require(
        old.skill_metadata.len() == metadata.len(),
        "invalid_catalog_state",
    )?;
    for (id, request) in &old.skill_requests {
        require(*id == request.request_id, "invalid_skill_request_state")?;
        let content = catalog_record(
            generation,
            source,
            CatalogContentV1::RetainedRequest {
                request_id: *id,
                requester: request.requester,
                name: request.name.clone(),
                role_ids: request.role_ids.clone(),
            },
            &mut records,
        );
        requests.insert(
            *id,
            SkillRequestCore {
                request_id: *id,
                requester: request.requester,
                content,
                role_ids: request.role_ids.clone(),
                status: request.status,
                skill_id: request.skill_id,
            },
        );
    }
    let mut reason_records = Vec::new();
    let mut reasons = Vec::new();
    for (index, reason) in old.reasons.iter().enumerate() {
        let record = ReasonMetadataRecordV1 {
            generation,
            provenance: ReasonProvenanceV1::Migration {
                source_archive_hash: source,
                record_index: u64::try_from(index).map_err(|_| Error::internal())?,
                origin: reason.origin,
                occurred_at: reason.occurred_at,
            },
            project_id: reason.project_id,
            milestone_id: reason.milestone_id,
            kind: reason.kind,
            reason: reason.reason.clone(),
        };
        reasons.push(ReasonCore {
            project_id: reason.project_id,
            milestone_id: reason.milestone_id,
            origin: reason.origin,
            content: ReasonReferenceV1::Migration(source_hash(&record.hash_input())),
            occurred_at: reason.occurred_at,
            kind: reason.kind,
        });
        reason_records.push(record);
    }
    let current = CoreArchiveV4 {
        version: 4,
        checkpoint: CoreCheckpointV4 {
            info: old.info,
            roles,
            skills,
            skill_metadata: metadata,
            skill_requests: requests,
            score_policy: old.score_policy,
            compensation_policy: old.compensation_policy,
            workers: old.workers,
            projects: old.projects,
            balances: old.balances,
            nonces: old.nonces,
            receipts: old.receipts,
            events: old.events,
            reasons,
            disputes: old.disputes,
            bramp: old.bramp,
            next_task_id: old.next_task_id,
            minted_units: old.minted_units,
            engine_version: 4,
        },
    };
    Ok((
        ReferenceState::from_archive(root, current)?.archive()?,
        records,
        reason_records,
    ))
}
fn catalog<'a>(
    records: &'a BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
    reference: &CatalogReferenceV1,
) -> Result<&'a CatalogMetadataRecordV1> {
    let record = records
        .get(&reference.hash())
        .ok_or_else(|| Error::domain("metadata_record_missing"))?;
    require(
        source_hash(&record.hash_input()) == reference.hash(),
        "invalid_metadata_record",
    )?;
    Ok(record)
}
/// Restore only this slice from current core facts and verified owned documents.
#[allow(
    clippy::too_many_lines,
    reason = "Keep the inverse field transfer and context verification together; no old engine runs."
)]
pub fn to_predecessor(
    archive: CoreArchiveV4,
    catalog_records: &BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
    reason_records: &BTreeMap<PayloadHash, ReasonMetadataRecordV1>,
) -> Result<CoreArchiveV3> {
    let saved = ReferenceState::from_archive(archive.checkpoint.info.root_account, archive)?
        .archive()?
        .checkpoint;
    let generation = saved.info.provider_instance_id;
    let root = saved.info.root_account;
    let request_context: Vec<_> = saved.skill_requests.values().cloned().collect();
    let mut roles = BTreeMap::new();
    let mut skills = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    let mut requests = BTreeMap::new();
    for (id, entry) in &saved.roles {
        let record = catalog(catalog_records, &entry.content)?;
        let name = record.entry_name(
            generation,
            CatalogKind::Role,
            *id,
            root,
            &entry.content,
            &request_context,
        )?;
        roles.insert(
            *id,
            CatalogEntry {
                id: *id,
                name: name.into(),
                fixed: entry.fixed,
            },
        );
    }
    for (id, entry) in &saved.skills {
        let record = catalog(catalog_records, &entry.content)?;
        let name = record.entry_name(
            generation,
            CatalogKind::Skill,
            *id,
            root,
            &entry.content,
            &request_context,
        )?;
        let origin = saved
            .skill_metadata
            .get(id)
            .ok_or_else(|| Error::domain("invalid_catalog_state"))?;
        let category = catalog(catalog_records, &origin.origin)?.category(
            generation,
            *id,
            root,
            &origin.origin,
            &request_context,
        )?;
        skills.insert(
            *id,
            CatalogEntry {
                id: *id,
                name: name.into(),
                fixed: entry.fixed,
            },
        );
        metadata.insert(
            *id,
            provider_execution::migration::SkillMetadata {
                category: category.into(),
                role_ids: origin.role_ids.clone(),
            },
        );
    }
    for (id, request) in &saved.skill_requests {
        let name = catalog(catalog_records, &request.content)?.request_name(generation, request)?;
        requests.insert(
            *id,
            SkillRequestView {
                request_id: *id,
                requester: request.requester,
                name: name.into(),
                role_ids: request.role_ids.clone(),
                status: request.status,
                skill_id: request.skill_id,
            },
        );
    }
    let mut reasons = Vec::new();
    for (index, reason) in saved.reasons.iter().enumerate() {
        let record = reason_records
            .get(&reason.content.hash())
            .ok_or_else(|| Error::domain("metadata_record_missing"))?;
        require(
            source_hash(&record.hash_input()) == reason.content.hash()
                && record.matches_context(
                    generation,
                    u64::try_from(index).map_err(|_| Error::internal())?,
                    reason,
                ),
            "invalid_metadata_record",
        )?;
        reasons.push(provider_execution::migration::ReasonRecord {
            project_id: reason.project_id,
            milestone_id: reason.milestone_id,
            origin: reason.origin,
            reason: record.reason.clone(),
            occurred_at: reason.occurred_at,
            kind: reason.kind,
        });
    }
    let old = CoreArchiveV3 {
        version: 3,
        checkpoint: CoreCheckpointV3 {
            info: saved.info,
            roles,
            skills,
            skill_metadata: metadata,
            skill_requests: requests,
            score_policy: saved.score_policy,
            compensation_policy: saved.compensation_policy,
            workers: saved.workers,
            projects: saved.projects,
            balances: saved.balances,
            nonces: saved.nonces,
            receipts: saved.receipts,
            events: saved.events,
            reasons,
            disputes: saved.disputes,
            bramp: saved.bramp,
            next_task_id: saved.next_task_id,
            minted_units: saved.minted_units,
            engine_version: 3,
        },
    };
    check_legacy_labels(&old.checkpoint)?;
    Ok(old)
}
/// Recover only successful legacy operations selected by current core refs.
/// Compact operation preimages still require the API's backup/export.
pub fn recover_legacy_proposals(archive: &CoreArchiveV4) -> Result<Vec<ProposalMetadataRecordV1>> {
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
pub fn recover_legacy_creations(archive: &CoreArchiveV4) -> Result<Vec<CreationMetadataRecordV1>> {
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

/// Recover selected seed/legacy catalog preimages only. Compact writes and
/// migration refs still require the API's owner backup, never guessed values.
pub fn recover_legacy_catalog(archive: &CoreArchiveV4) -> Result<Vec<CatalogMetadataRecordV1>> {
    let saved =
        ReferenceState::from_archive(archive.checkpoint.info.root_account, archive.clone())?
            .archive()?
            .checkpoint;
    let generation = saved.info.provider_instance_id;
    let mut records = BTreeMap::new();
    let selected: BTreeSet<_> = saved
        .roles
        .values()
        .chain(saved.skills.values())
        .map(|e| e.content.hash())
        .chain(saved.skill_metadata.values().map(|s| s.origin.hash()))
        .chain(saved.skill_requests.values().map(|r| r.content.hash()))
        .collect();
    for seed in generated_contracts::provider_core::catalog_seed_v1() {
        let record = seed.record(generation);
        let key = source_hash(&record.hash_input());
        if selected.contains(&key) {
            records.insert(key, record);
        }
    }
    for recorded in saved
        .receipts
        .values()
        .filter(|r| r.receipt.outcome == ExecutionOutcome::Success)
    {
        let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
        if let Some(record) = CatalogMetadataRecordV1::from_command(
            generation,
            call.origin,
            call.operation_id,
            &call.command,
        ) {
            let key = source_hash(&record.hash_input());
            if selected.contains(&key) {
                records.insert(key, record);
            }
        }
    }
    Ok(records.into_values().collect())
}
/// Recover selected successful legacy lifecycle text, preserving actual order.
pub fn recover_legacy_reasons(archive: &CoreArchiveV4) -> Result<Vec<ReasonMetadataRecordV1>> {
    let saved =
        ReferenceState::from_archive(archive.checkpoint.info.root_account, archive.clone())?
            .archive()?
            .checkpoint;
    let generation = saved.info.provider_instance_id;
    let mut records = Vec::new();
    for (index, core) in saved.reasons.iter().enumerate() {
        let ReasonReferenceV1::Write(content) = &core.content else {
            continue;
        };
        let retained = saved
            .receipts
            .get(&content.operation_id)
            .ok_or_else(|| Error::domain("invalid_reason_metadata_state"))?;
        let call = UnsignedContractCallV1::decode_signable(&retained.bytes)?;
        if let Some(record) = ReasonMetadataRecordV1::from_command(
            generation,
            call.origin,
            call.operation_id,
            &call.command,
        ) {
            require(
                source_hash(&record.hash_input()) == content.hash
                    && record.matches_context(
                        generation,
                        u64::try_from(index).map_err(|_| Error::internal())?,
                        core,
                    ),
                "invalid_reason_metadata_state",
            )?;
            records.push(record);
        }
    }
    Ok(records)
}
