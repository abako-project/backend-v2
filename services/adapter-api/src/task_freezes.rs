//! Immutable accepted task snapshots and durable project edit holds.
use crate::state::{App, Error, parse};
use generated_contracts::{
    EntityId, ExecutionOutcome, OperationId, OperationReceipt, ProviderCommand, ProviderInstanceId,
    TaskStorageView,
    provider_core::{TaskStateV1, TaskStorageRecordV1},
};
use parity_scale_codec::Encode;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::BTreeMap;

pub(crate) fn kind(command: &ProviderCommand) -> Option<&'static str> {
    match command {
        ProviderCommand::CancelProject { .. } | ProviderCommand::CancelProjectCore { .. } => {
            Some("Cancellation")
        }
        ProviderCommand::OpenDispute(_)
        | ProviderCommand::OpenDisputeWithComment(_)
        | ProviderCommand::OpenDisputeCommitted { .. } => Some("Dispute"),
        // Planning disputes do not currently freeze task execution. Preserve that
        // behavior pending the separate owner decision recorded in the handoff.
        _ => None,
    }
}
fn task_project(command: &ProviderCommand) -> Option<EntityId> {
    match command {
        ProviderCommand::CreateTask { project_id, .. }
        | ProviderCommand::EditTask { project_id, .. }
        | ProviderCommand::DeleteTask { project_id, .. }
        | ProviderCommand::UpdateTaskProgress { project_id, .. } => Some(*project_id),
        _ => None,
    }
}

/// Upgrade only queued API-owned work. Unknown future provider-owned histories
/// stay untouched, and large command decoding has bounded pages off the reactor.
pub(crate) async fn backfill_context(connection: &mut PgConnection) -> Result<(), Error> {
    let mut after = 0_i64;
    loop {
        let rows=sqlx::query("SELECT o.operation_id,o.creation_sequence,o.command_json,o.command_bytes,o.request_command_bytes FROM operations o JOIN task_generations g ON g.provider_instance_id=o.provider_instance_id AND g.api_owned WHERE o.task_operation AND o.status='Queued' AND o.task_project_id IS NULL AND o.creation_sequence>$1 ORDER BY o.creation_sequence LIMIT 64 FOR UPDATE OF o")
            .bind(after).fetch_all(&mut *connection).await?;
        if rows.is_empty() {
            return Ok(());
        }
        let projects = tokio::task::spawn_blocking(move || {
            rows.into_iter()
                .map(|row| {
                    let command: ProviderCommand =
                        serde_json::from_str(row.try_get("command_json")?)?;
                    let project = task_project(&command).ok_or(Error::Dependency)?;
                    let bytes = command.encode();
                    if bytes != row.try_get::<Vec<u8>, _>("command_bytes")?
                        || Some(bytes)
                            != row.try_get::<Option<Vec<u8>>, _>("request_command_bytes")?
                    {
                        return Err(Error::Dependency);
                    }
                    Ok::<_, Error>((
                        row.try_get::<String, _>("operation_id")?,
                        row.try_get::<i64, _>("creation_sequence")?,
                        project,
                    ))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await??;
        for (id, sequence, project) in projects {
            sqlx::query("UPDATE operations SET task_project_id=$2 WHERE operation_id=$1 AND task_project_id IS NULL")
                .bind(id).bind(project.to_string()).execute(&mut *connection).await?;
            after = sequence;
        }
    }
}

/// Operation lock precedes project lock; the non-key allocator lock is compatible
/// with foreign-key readers when first writes arrive on different projects.
pub(crate) async fn lock_project(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    project: EntityId,
) -> Result<Option<u32>, Error> {
    sqlx::query("INSERT INTO task_projects(provider_instance_id,project_id) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM task_generations WHERE provider_instance_id=$1 AND api_owned) ON CONFLICT DO NOTHING")
        .bind(generation.to_string()).bind(project.to_string()).execute(&mut *connection).await?;
    let locked:Option<String>=sqlx::query_scalar("SELECT project_id FROM task_projects WHERE provider_instance_id=$1 AND project_id=$2 FOR UPDATE")
        .bind(generation.to_string()).bind(project.to_string()).fetch_optional(&mut *connection).await?;
    let row=sqlx::query("SELECT next_task_id,api_owned FROM task_generations WHERE provider_instance_id=$1 FOR NO KEY UPDATE")
        .bind(generation.to_string()).fetch_optional(&mut *connection).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if !row.try_get::<bool, _>("api_owned")? {
        return Ok(None);
    }
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
    let Some(next) = lock_project(connection, generation, project).await? else {
        return Ok(());
    };
    let snapshot = app.snapshot().await?;
    if snapshot.info.provider_instance_id != generation {
        return Err(Error::Dependency);
    }
    let core = snapshot
        .projects
        .into_iter()
        .find(|p| p.project_id == project)
        .ok_or(Error::NotFound)?;
    let ids = core
        .proposals
        .iter()
        .flat_map(|p| &p.milestones)
        .map(|m| m.task_storage.task_storage_id.to_string())
        .collect::<Vec<_>>();
    let storages=sqlx::query("SELECT task_storage_id,project_id,proposal_id,milestone_id FROM task_storages WHERE provider_instance_id=$1 AND task_storage_id=ANY($2)")
        .bind(generation.to_string()).bind(&ids).fetch_all(&mut *connection).await?;
    let rows=sqlx::query("SELECT task_id,task_storage_id,ordinal,payload,assignees FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=ANY($2) ORDER BY task_storage_id,ordinal")
        .bind(generation.to_string()).bind(&ids).fetch_all(&mut *connection).await?;
    let json = tokio::task::spawn_blocking(move || {
        let mut contexts = BTreeMap::new();
        for row in storages {
            contexts.insert(
                parse::<EntityId>(row.try_get("task_storage_id")?)?,
                (
                    parse::<EntityId>(row.try_get("project_id")?)?,
                    parse::<EntityId>(row.try_get("proposal_id")?)?,
                    parse::<EntityId>(row.try_get("milestone_id")?)?,
                ),
            );
        }
        let mut grouped = BTreeMap::<EntityId, Vec<PgRow>>::new();
        for row in rows {
            grouped
                .entry(parse(row.try_get("task_storage_id")?)?)
                .or_default()
                .push(row);
        }
        let mut captured = Vec::new();
        for proposal in core.proposals {
            for milestone in proposal.milestones {
                let id = milestone.task_storage.task_storage_id;
                match contexts.remove(&id) {
                    Some(context)
                        if context == (project, proposal.proposal_id, milestone.milestone_id) => {}
                    None if milestone.task_storage.tasks.is_empty() => {}
                    _ => return Err(Error::Dependency),
                }
                let tasks = crate::task_execution::decode_tasks(
                    grouped.remove(&id).unwrap_or_default(),
                    next,
                )?;
                captured.push(TaskStorageRecordV1 {
                    project_id: project,
                    proposal_id: proposal.proposal_id,
                    proposal_revision: proposal.revision,
                    storage: TaskStorageView {
                        task_storage_id: id,
                        milestone_id: milestone.milestone_id,
                        tasks,
                    },
                });
            }
        }
        if !contexts.is_empty() || !grouped.is_empty() {
            return Err(Error::Dependency);
        }
        captured.sort_by_key(|r| r.storage.task_storage_id);
        let state = TaskStateV1 {
            schema_version: 1,
            generation,
            next_task_id: next,
            storages: captured,
        };
        state.validate().map_err(|_| Error::Dependency)?;
        Ok::<_, Error>(serde_json::to_string(&state.storages)?)
    })
    .await??;
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
/// owned. API-owned cases require proved acceptance data; absence is not evidence
/// that a case predates cutover or that current/provider task text is historical.
pub(crate) async fn compose_case(
    app: &App,
    generation: ProviderInstanceId,
    proposal: EntityId,
    view: generated_contracts::provider_core::CoreDisputeView,
) -> Result<generated_contracts::provider_core::CoreDisputeView, Error> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let Some(next) = crate::task_views::counter(&mut tx, generation).await? else {
        tx.commit().await?;
        return Ok(view);
    };
    // Core currently permits one permanent milestone dispute per project. Two
    // confirmed opening records are ambiguous and cannot establish frozen text.
    let rows=sqlx::query("SELECT f.operation_id,f.snapshot_json,o.command_json,o.command_bytes,o.provider_instance_id,o.account_id,o.signable_payload,o.signed_json,o.receipt_json FROM task_freezes f JOIN operations o USING(operation_id) WHERE f.provider_instance_id=$1 AND f.project_id=$2 AND f.kind='Dispute' AND f.state='Confirmed' AND o.status='Finalized' AND NOT o.task_operation ORDER BY o.creation_sequence LIMIT 2")
        .bind(generation.to_string()).bind(view.dispute.project_id.to_string()).fetch_all(&mut *tx).await?;
    let view = tokio::task::spawn_blocking(move || {
        frozen(generation, proposal, next, view, rows).map_err(|_| Error::Dependency)
    })
    .await??;
    tx.commit().await?;
    Ok(view)
}
fn frozen(
    generation: ProviderInstanceId,
    proposal: EntityId,
    next: u32,
    mut view: generated_contracts::provider_core::CoreDisputeView,
    mut rows: Vec<PgRow>,
) -> Result<generated_contracts::provider_core::CoreDisputeView, Error> {
    use generated_contracts::{
        EvidenceRecordReferenceV1, SignedContractCallV1, UnsignedContractCallV1,
    };
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
    let state = TaskStateV1 {
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
    {
        return Err(Error::Dependency);
    }
    view.milestone.task_storage.tasks = record.storage.tasks;
    Ok(view)
}
