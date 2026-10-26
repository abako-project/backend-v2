//! Explicit predecessor case capture; never infer frozen text from current edits.
use crate::state::{App, Error, hash};
use generated_contracts::{
    Dispute, EntityId, ProviderInstanceId, TaskStorageView,
    provider_core::{
        CoreDisputeView, CoreDisputeViewV4, CoreSnapshotV4, TaskStateV1, TaskStorageCore,
        TaskStorageRecordV1,
    },
};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

/// Verify both directions: a missing core case must not silently discard an
/// immutable API snapshot during activation or rollback.
pub(crate) async fn verify_inventory(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    actual: Vec<EntityId>,
) -> Result<(), Error> {
    let migrated: Vec<String> = sqlx::query_scalar(
        "SELECT case_id FROM task_migration_cases WHERE provider_instance_id=$1",
    )
    .bind(generation.to_string())
    .fetch_all(&mut *connection)
    .await?;
    let accepted: Vec<(String,Option<String>)> = sqlx::query_as("SELECT f.operation_id,o.receipt_json FROM task_freezes f JOIN operations o USING(operation_id) WHERE f.provider_instance_id=$1 AND f.kind='Dispute' AND f.state='Confirmed'")
        .bind(generation.to_string()).fetch_all(connection).await?;
    tokio::task::spawn_blocking(move || {
        let mut expected = std::collections::BTreeSet::new();
        for id in migrated {
            expected.insert(crate::state::parse::<EntityId>(&id)?);
        }
        for (operation, receipt) in accepted {
            let receipt: generated_contracts::OperationReceipt =
                serde_json::from_str(receipt.as_deref().ok_or(Error::Dependency)?)?;
            if receipt.provider_instance_id != generation
                || receipt.operation_id.to_string() != operation
                || receipt.outcome != generated_contracts::ExecutionOutcome::Success
                || !expected.insert(receipt.created_entity_id.ok_or(Error::Dependency)?)
            {
                return Err(Error::Dependency);
            }
        }
        let count = actual.len();
        let actual: std::collections::BTreeSet<_> = actual.into_iter().collect();
        if actual.len() != count || actual != expected {
            return Err(Error::Conflict("task_case_inventory_mismatch"));
        }
        Ok(())
    })
    .await?
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Captured {
    generation: ProviderInstanceId,
    case_id: EntityId,
    opening_hash: [u8; 32],
    source_hash: [u8; 32],
    tasks: TaskStorageRecordV1,
}

fn opening(case: &Dispute) -> Result<[u8; 32], Error> {
    // Responses and later adjudication status are mutable. Only the immutable
    // opening context belongs in the migration record's identity.
    let mut bytes = b"abako/task-case-opening/v1\0".to_vec();
    serde_json::to_writer(
        &mut bytes,
        &(
            case.dispute_id,
            case.project_id,
            case.milestone_id,
            case.rejected_submission_id,
            case.opened_by,
            case.counterparty,
            case.opened_at,
            &case.evidence,
            case.opening_comment_id,
            case.proposal_revision,
            case.context_event_cursor,
        ),
    )?;
    Ok(hash(&bytes))
}

/// The caller holds the paused-owner locks and has verified the prepared task
/// fingerprint. HTTP accesses only the predecessor's public private protocol.
pub(crate) async fn capture(
    app: &App,
    connection: &mut PgConnection,
    state: TaskStateV1,
    source_hash: [u8; 32],
) -> Result<usize, Error> {
    let snapshot: CoreSnapshotV4 =
        crate::task_rollback::fetch_target(app, "/internal/snapshot").await?;
    let generation = state.generation;
    let cases = tokio::task::spawn_blocking(move || {
        if snapshot.schema_version != 4 || snapshot.info.provider_instance_id != generation {
            return Err(Error::Dependency);
        }
        let mut actual = Vec::new();
        let mut cases = Vec::new();
        let mut expected_cases = std::collections::BTreeSet::new();
        for project in snapshot.projects {
            if let Some(id) = project.active_dispute_id {
                expected_cases.insert(id);
            }
            for proposal in project.proposals {
                for milestone in proposal.milestones {
                    let record = TaskStorageRecordV1 {
                        project_id: project.project_id,
                        proposal_id: proposal.proposal_id,
                        proposal_revision: proposal.revision,
                        storage: milestone.task_storage.clone(),
                    };
                    actual.push(record.clone());
                    if let Some(id) = project.active_dispute_id
                        && milestone.frozen
                    {
                        cases.push((id, milestone, record));
                    }
                }
            }
        }
        actual.sort_by_key(|r| r.storage.task_storage_id);
        if actual != state.storages || cases.len() != expected_cases.len() {
            return Err(Error::Dependency);
        }
        Ok(cases)
    })
    .await??;
    let count = cases.len();
    for (id, milestone, record) in cases {
        let view: CoreDisputeViewV4 =
            crate::task_rollback::fetch_target(app, &format!("/internal/disputes/{id}")).await?;
        let (payload, digest) = tokio::task::spawn_blocking(move || {
            if view.dispute.dispute_id != id
                || view.dispute.project_id != record.project_id
                || view.dispute.proposal_revision != record.proposal_revision
                || view.dispute.milestone_id != milestone.milestone_id
                || view.milestone != milestone
            {
                return Err(Error::Dependency);
            }
            let captured = Captured {
                generation,
                case_id: id,
                opening_hash: opening(&view.dispute)?,
                source_hash,
                tasks: record,
            };
            let payload = serde_json::to_string(&captured)?;
            let digest = hash(payload.as_bytes());
            Ok::<_, Error>((payload, digest))
        })
        .await??;
        sqlx::query("INSERT INTO task_migration_cases(provider_instance_id,case_id,payload,payload_hash) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(generation.to_string()).bind(id.to_string()).bind(&payload).bind(digest.as_slice()).execute(&mut *connection).await?;
        let saved: (String,Vec<u8>) = sqlx::query_as("SELECT payload,payload_hash FROM task_migration_cases WHERE provider_instance_id=$1 AND case_id=$2")
            .bind(generation.to_string()).bind(id.to_string()).fetch_one(&mut *connection).await?;
        tokio::task::spawn_blocking(move || {
            if saved.0 != payload || saved.1 != digest {
                return Err(Error::Conflict("task_migration_case_conflict"));
            }
            Ok::<_, Error>(())
        })
        .await??;
    }
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_migration_cases WHERE provider_instance_id=$1",
    )
    .bind(generation.to_string())
    .fetch_one(connection)
    .await?;
    if usize::try_from(retained).map_err(|_| Error::Dependency)? != count {
        return Err(Error::Conflict("task_migration_case_conflict"));
    }
    Ok(count)
}

/// A missing record remains missing. This function never captures current text.
pub(crate) async fn lookup(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    proposal: EntityId,
    next: u32,
    view: CoreDisputeView,
) -> Result<Option<TaskStorageView>, Error> {
    let row: Option<(String,Vec<u8>,Vec<u8>)> = sqlx::query_as("SELECT c.payload,c.payload_hash,g.source_hash FROM task_migration_cases c JOIN task_generations g USING(provider_instance_id) WHERE c.provider_instance_id=$1 AND c.case_id=$2")
        .bind(generation.to_string()).bind(view.dispute.dispute_id.to_string()).fetch_optional(connection).await?;
    let Some((payload, digest, source_hash)) = row else {
        return Ok(None);
    };
    tokio::task::spawn_blocking(move || {
        if payload.len() > 33_554_432 || hash(payload.as_bytes()).as_slice() != digest {
            return Err(Error::Dependency);
        }
        let captured: Captured = serde_json::from_str(&payload)?;
        let record = &captured.tasks;
        if captured.source_hash.as_slice() != source_hash
            || captured.generation != generation
            || captured.case_id != view.dispute.dispute_id
            || captured.opening_hash != opening(&view.dispute)?
            || record.project_id != view.dispute.project_id
            || record.proposal_id != proposal
            || record.proposal_revision != view.dispute.proposal_revision
            || record.storage.milestone_id != view.dispute.milestone_id
            || TaskStorageCore::from(&record.storage) != view.milestone.task_storage
        {
            return Err(Error::Dependency);
        }
        TaskStateV1 {
            schema_version: 1,
            generation,
            next_task_id: next,
            storages: vec![captured.tasks.clone()],
        }
        .validate()
        .map_err(|_| Error::Dependency)?;
        Ok(Some(captured.tasks.storage))
    })
    .await?
}
