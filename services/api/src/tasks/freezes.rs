//! Immutable accepted task snapshots and durable project edit holds.
use crate::state::{App, Error, parse};
use parity_scale_codec::Encode;
use protocol::{
    EntityId, ExecutionOutcome, OperationId, OperationReceipt, ProviderCommand, ProviderInstanceId,
    TaskStorageView, provider_core::TaskSnapshot,
};
use sqlx::{PgConnection, Row, postgres::PgRow};

pub(crate) fn kind(command: &ProviderCommand) -> Option<&'static str> {
    match command {
        ProviderCommand::CancelProject { .. } | ProviderCommand::CancelProjectCore { .. } => {
            Some("Cancellation")
        }
        ProviderCommand::OpenDispute(_)
        | ProviderCommand::OpenDisputeWithComment(_)
        | ProviderCommand::OpenDisputeCommitted { .. } => Some("Dispute"),
        // Planning disputes do not currently freeze task execution. Preserve that
        // behavior under the owner-approved task policy.
        _ => None,
    }
}
pub(crate) fn task_project(command: &ProviderCommand) -> Option<EntityId> {
    match command {
        ProviderCommand::CreateTask { project_id, .. }
        | ProviderCommand::EditTask { project_id, .. }
        | ProviderCommand::DeleteTask { project_id, .. }
        | ProviderCommand::UpdateTaskProgress { project_id, .. } => Some(*project_id),
        _ => None,
    }
}

/// Operation lock precedes project lock; the non-key allocator lock is compatible
/// with foreign-key readers when first writes arrive on different projects.
pub(crate) async fn lock_project(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    project: EntityId,
) -> Result<Option<u32>, Error> {
    sqlx::query("INSERT INTO task_projects(provider_instance_id,project_id) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM task_generations WHERE provider_instance_id=$1) ON CONFLICT DO NOTHING")
        .bind(generation.to_string()).bind(project.to_string()).execute(&mut *connection).await?;
    let locked:Option<String>=sqlx::query_scalar("SELECT project_id FROM task_projects WHERE provider_instance_id=$1 AND project_id=$2 FOR UPDATE")
        .bind(generation.to_string()).bind(project.to_string()).fetch_optional(&mut *connection).await?;
    let row = sqlx::query(
        "SELECT next_task_id FROM task_generations WHERE provider_instance_id=$1 FOR NO KEY UPDATE",
    )
    .bind(generation.to_string())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if locked.is_none() {
        return Err(Error::Dependency);
    }
    Ok(Some(
        u32::try_from(row.try_get::<i64, _>("next_task_id")?).map_err(|_| Error::Dependency)?,
    ))
}

pub(crate) enum Hold {
    Clear,
    Pending,
    Confirmed(&'static str),
}
pub(crate) async fn hold(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    project: EntityId,
) -> Result<Hold, Error> {
    let row=sqlx::query("SELECT COALESCE(bool_or(state='Confirmed' AND kind='Cancellation'),FALSE) AS cancelled,COALESCE(bool_or(state='Confirmed'),FALSE) AS confirmed,COALESCE(bool_or(state='Pending'),FALSE) AS pending FROM task_freezes WHERE provider_instance_id=$1 AND project_id=$2 AND state<>'Released'")
        .bind(generation.to_string()).bind(project.to_string()).fetch_one(connection).await?;
    Ok(if row.try_get("cancelled")? {
        Hold::Confirmed("project_cancelled")
    } else if row.try_get("confirmed")? {
        Hold::Confirmed("project_disputed")
    } else if row.try_get("pending")? {
        Hold::Pending
    } else {
        Hold::Clear
    })
}

/// Called after queue insertion in the same admission transaction. Replays return
/// before this function, so acceptance can never replace an earlier snapshot.
pub(crate) async fn capture(
    app: &App,
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    operation: OperationId,
    command: &ProviderCommand,
) -> Result<(), Error> {
    let Some(kind) = kind(command) else {
        return Ok(());
    };
    let project = command.project_id().ok_or(Error::Dependency)?;
    let next = lock_project(connection, generation, project)
        .await?
        .ok_or(Error::Dependency)?;
    let snapshot = app.snapshot().await?;
    if snapshot.info.provider_instance_id != generation {
        return Err(Error::Dependency);
    }
    let core = snapshot
        .projects
        .into_iter()
        .find(|p| p.project_id == project)
        .ok_or(Error::NotFound)?;
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE provider_instance_id=$1 AND task_project_id=$2 AND NOT task_operation AND (status NOT IN ('Finalized','Rejected','Expired') OR (status='Finalized' AND receipt_json IS NULL) OR (possibly_submitted AND (status='Expired' OR (status='Rejected' AND error_code IS DISTINCT FROM 'provider_rejected')))))")
        .bind(generation.to_string()).bind(project.to_string()).fetch_one(&mut *connection).await?;
    if pending {
        return Err(Error::Conflict("task_core_pending"));
    }
    let tasks =
        crate::tasks::views::current_state(connection, generation, next, vec![core]).await?;
    let json =
        tokio::task::spawn_blocking(move || serde_json::to_string(&tasks.storages)).await??;
    sqlx::query("INSERT INTO task_freezes(operation_id,provider_instance_id,project_id,kind,snapshot_json) VALUES($1,$2,$3,$4,$5)")
        .bind(operation.to_string()).bind(generation.to_string()).bind(project.to_string()).bind(kind).bind(json).execute(connection).await?;
    Ok(())
}

/// The caller has already fenced and locked this operation in `operations::save`.
/// Never lock another operation after taking the project lock.
pub(crate) async fn complete(
    connection: &mut PgConnection,
    operation: OperationId,
) -> Result<(), Error> {
    let row=sqlx::query("SELECT f.provider_instance_id AS generation,f.project_id,f.kind,o.provider_instance_id,o.account_id,o.command_json,o.status,o.possibly_submitted,o.error_code,o.receipt_json FROM task_freezes f JOIN operations o USING(operation_id) WHERE f.operation_id=$1 AND f.state='Pending'")
        .bind(operation.to_string()).fetch_optional(&mut *connection).await?;
    let Some(row) = row else {
        return Ok(());
    };
    let generation: ProviderInstanceId = parse(row.try_get("generation")?)?;
    let project: EntityId = parse(row.try_get("project_id")?)?;
    let command: ProviderCommand = serde_json::from_str(row.try_get("command_json")?)?;
    if kind(&command) != Some(row.try_get("kind")?)
        || command.project_id() != Some(project)
        || row.try_get::<Option<&str>, _>("provider_instance_id")?
            != Some(generation.to_string().as_str())
    {
        return Err(Error::Dependency);
    }
    let state = match row.try_get::<&str, _>("status")? {
        "Finalized" => {
            let Some(json) = row.try_get::<Option<&str>, _>("receipt_json")? else {
                return Ok(());
            };
            let receipt: OperationReceipt = serde_json::from_str(json)?;
            if receipt.operation_id != operation
                || receipt.provider_instance_id != generation
                || receipt.origin.to_string() != row.try_get::<&str, _>("account_id")?
            {
                return Err(Error::Dependency);
            }
            if receipt.outcome == ExecutionOutcome::Success {
                "Confirmed"
            } else {
                "Released"
            }
        }
        "Rejected"
            if !row.try_get::<bool, _>("possibly_submitted")?
                || row.try_get::<Option<&str>, _>("error_code")? == Some("provider_rejected") =>
        {
            "Released"
        }
        "Expired" if !row.try_get::<bool, _>("possibly_submitted")? => "Released",
        _ => return Ok(()),
    };
    sqlx::query("SELECT project_id FROM task_projects WHERE provider_instance_id=$1 AND project_id=$2 FOR UPDATE")
        .bind(generation.to_string()).bind(project.to_string()).fetch_one(&mut *connection).await?;
    sqlx::query("UPDATE task_freezes SET state=$2 WHERE operation_id=$1 AND state='Pending'")
        .bind(operation.to_string())
        .bind(state)
        .execute(connection)
        .await?;
    Ok(())
}

/// Public frozen presentation only. Raw private-case authorization remains core
/// owned. Cases require verified acceptance data; absent snapshots fail closed.
pub(crate) async fn compose_case(
    app: &App,
    generation: ProviderInstanceId,
    proposal: EntityId,
    view: protocol::provider_core::CoreDisputeView,
) -> Result<TaskStorageView, Error> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let Some(next) = crate::tasks::views::counter(&mut tx, generation).await? else {
        return Err(Error::Dependency);
    };
    let view = accepted_case(&mut tx, generation, proposal, next, view).await?;
    tx.commit().await?;
    Ok(view)
}

pub(crate) async fn accepted_case(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    proposal: EntityId,
    next: u32,
    view: protocol::provider_core::CoreDisputeView,
) -> Result<TaskStorageView, Error> {
    // Core currently permits one permanent milestone dispute per project. Two
    // confirmed opening records are ambiguous and cannot establish frozen text.
    let rows=sqlx::query("SELECT f.operation_id,f.snapshot_json,o.command_json,o.command_bytes,o.provider_instance_id,o.account_id,o.signable_payload,o.signed_json,o.receipt_json FROM task_freezes f JOIN operations o USING(operation_id) WHERE f.provider_instance_id=$1 AND f.project_id=$2 AND f.kind='Dispute' AND f.state='Confirmed' AND o.status='Finalized' AND NOT o.task_operation ORDER BY o.creation_sequence LIMIT 2")
        .bind(generation.to_string()).bind(view.dispute.project_id.to_string()).fetch_all(&mut *connection).await?;
    let view = tokio::task::spawn_blocking(move || {
        frozen(generation, proposal, next, &view, rows).map_err(|_| Error::Dependency)
    })
    .await??;
    Ok(view)
}
fn frozen(
    generation: ProviderInstanceId,
    proposal: EntityId,
    next: u32,
    view: &protocol::provider_core::CoreDisputeView,
    mut rows: Vec<PgRow>,
) -> Result<TaskStorageView, Error> {
    use protocol::{EvidenceRecordReferenceV1, SignedContractCallV1, UnsignedContractCallV1};
    if rows.len() != 1 {
        return Err(Error::Dependency);
    }
    let row = rows.pop().ok_or(Error::Dependency)?;
    let operation: OperationId = parse(row.try_get("operation_id")?)?;
    let command: ProviderCommand = serde_json::from_str(row.try_get("command_json")?)?;
    let receipt: OperationReceipt = serde_json::from_str(row.try_get("receipt_json")?)?;
    let call = UnsignedContractCallV1::decode_signable(row.try_get("signable_payload")?)
        .map_err(|_| Error::Dependency)?;
    let signed: SignedContractCallV1 = serde_json::from_str(row.try_get("signed_json")?)?;
    let case = &view.dispute;
    if row.try_get::<Option<&str>, _>("provider_instance_id")?
        != Some(generation.to_string().as_str())
        || receipt.operation_id != operation
        || receipt.provider_instance_id != generation
        || receipt.outcome != ExecutionOutcome::Success
        || receipt.created_entity_id != Some(case.dispute_id)
        || receipt.origin != case.opened_by
        || receipt.finalized_at != case.opened_at
        || receipt.origin.to_string() != row.try_get::<&str, _>("account_id")?
        || call.operation_id != operation
        || call.provider_instance_id != generation
        || call.origin != receipt.origin
        || call.nonce != receipt.nonce
        || call.command != command
        || signed.call != call
        || command.encode() != row.try_get::<Vec<u8>, _>("command_bytes")?
    {
        return Err(Error::Dependency);
    }
    let (project, milestone, submission) = match &command {
        ProviderCommand::OpenDisputeCommitted {
            project_id,
            milestone_id,
            rejected_submission_id,
            evidence,
        } => {
            if case.evidence != Some(EvidenceRecordReferenceV1::Committed(evidence.clone())) {
                return Err(Error::Dependency);
            }
            (*project_id, *milestone_id, *rejected_submission_id)
        }
        ProviderCommand::OpenDispute(request) => {
            if case.evidence != Some(request.evidence.clone().into()) {
                return Err(Error::Dependency);
            }
            (
                request.project_id,
                request.milestone_id,
                request.rejected_submission_id,
            )
        }
        ProviderCommand::OpenDisputeWithComment(request) => {
            if case.opening_comment_id != Some(request.comment_id) {
                return Err(Error::Dependency);
            }
            (
                request.project_id,
                request.milestone_id,
                request.rejected_submission_id,
            )
        }
        _ => return Err(Error::Dependency),
    };
    if (project, milestone, submission)
        != (
            case.project_id,
            case.milestone_id,
            case.rejected_submission_id,
        )
    {
        return Err(Error::Dependency);
    }
    let state = TaskSnapshot {
        schema_version: 1,
        generation,
        next_task_id: next,
        storages: serde_json::from_str(row.try_get("snapshot_json")?)?,
    };
    state.validate().map_err(|_| Error::Dependency)?;
    if state.storages.iter().any(|r| r.project_id != project) {
        return Err(Error::Dependency);
    }
    let record = state
        .storages
        .into_iter()
        .find(|r| r.proposal_id == proposal && r.storage.milestone_id == milestone)
        .ok_or(Error::Dependency)?;
    if record.proposal_revision != case.proposal_revision
        || record.storage.task_storage_id != view.milestone.task_storage.task_storage_id
        || view.milestone.task_storage.milestone_id != milestone
        || protocol::provider_core::TaskStorageCore::from(&record.storage)
            != view.milestone.task_storage
    {
        return Err(Error::Dependency);
    }
    Ok(record.storage)
}
