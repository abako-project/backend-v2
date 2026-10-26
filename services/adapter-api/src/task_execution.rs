//! Classify API content edits and core task membership work in the existing queue.
use crate::state::{App, Error, now, parse};
use generated_contracts::{
    AccountId32, AdapterTaskEvent, EntityId, ExecutionOutcome, OperationId, ProviderCommand,
    ProviderInstanceId, TaskEventKind, TaskEventSource, TaskOperationResult, TaskView, UnixSeconds,
    provider_core::ProjectCore,
};
use parity_scale_codec::{DecodeAll, Encode};
use sqlx::{Row, postgres::PgRow};
use std::collections::BTreeSet;

/// The selected core command contains identity/membership only. The original
/// request remains in the existing operation row for eventual API materialization.
pub(crate) fn compact_command(request: &ProviderCommand) -> Option<ProviderCommand> {
    Some(match request {
        ProviderCommand::CreateTask {
            project_id,
            task_storage_id,
            task,
        } => ProviderCommand::CreateTaskCore {
            project_id: *project_id,
            task_storage_id: *task_storage_id,
            assignees: task.assignees.clone(),
        },
        ProviderCommand::EditTask {
            project_id,
            task_storage_id,
            task_id,
            task,
        } => ProviderCommand::SetTaskAssigneesCore {
            project_id: *project_id,
            task_storage_id: *task_storage_id,
            task_id: *task_id,
            assignees: task.assignees.clone(),
        },
        ProviderCommand::DeleteTask { .. } => request.clone(),
        _ => return None,
    })
}

/// Signing must never reinterpret an original request or a different project's
/// classified row. This is also used by the final materialization transaction.
pub(crate) fn provider_intent(
    row: &PgRow,
    command: &ProviderCommand,
) -> Result<Option<ProviderCommand>, Error> {
    let project: Option<&str> = row.try_get("task_project_id")?;
    let Some(project) = project else {
        return if matches!(
            command,
            ProviderCommand::CreateTaskCore { .. } | ProviderCommand::SetTaskAssigneesCore { .. }
        ) {
            Err(Error::Dependency)
        } else {
            Ok(None)
        };
    };
    let bytes: Vec<u8> = row
        .try_get::<Option<Vec<u8>>, _>("request_command_bytes")?
        .ok_or(Error::Dependency)?;
    if bytes.len() > generated_contracts::MAX_SIGNABLE_BYTES - 128 {
        return Err(Error::Dependency);
    }
    let original = ProviderCommand::decode_all(&mut &bytes[..]).map_err(|_| Error::Dependency)?;
    original.validate().map_err(|_| Error::Dependency)?;
    if original.project_id() != Some(parse(project)?)
        || original.encode() != bytes
        || compact_command(&original).as_ref() != Some(command)
        || command.encode() != row.try_get::<Vec<u8>, _>("command_bytes")?
    {
        return Err(Error::Dependency);
    }
    Ok(Some(original))
}

pub(crate) struct Job {
    id: OperationId,
    lease: String,
}
impl Job {
    pub(crate) fn new(id: OperationId, lease: String) -> Self {
        Self { id, lease }
    }
}
struct Request {
    id: OperationId,
    generation: ProviderInstanceId,
    actor: AccountId32,
    project: EntityId,
    storage: EntityId,
    command: ProviderCommand,
}
impl Request {
    fn read(row: &PgRow) -> Result<Self, Error> {
        let command: ProviderCommand = serde_json::from_str(row.try_get("command_json")?)?;
        command.validate().map_err(|_| Error::Dependency)?;
        let (project, storage) = match &command {
            ProviderCommand::CreateTask {
                project_id,
                task_storage_id,
                ..
            }
            | ProviderCommand::EditTask {
                project_id,
                task_storage_id,
                ..
            }
            | ProviderCommand::UpdateTaskProgress {
                project_id,
                task_storage_id,
                ..
            }
            | ProviderCommand::DeleteTask {
                project_id,
                task_storage_id,
                ..
            } => (*project_id, *task_storage_id),
            _ => return Err(Error::Dependency),
        };
        let bytes = command.encode();
        if row.try_get::<Option<&str>, _>("task_project_id")? != Some(project.to_string().as_str())
            || bytes != row.try_get::<Vec<u8>, _>("command_bytes")?
            || Some(bytes) != row.try_get::<Option<Vec<u8>>, _>("request_command_bytes")?
        {
            return Err(Error::Dependency);
        }
        Ok(Self {
            id: parse(row.try_get("operation_id")?)?,
            generation: parse(row.try_get("provider_instance_id")?)?,
            actor: parse(row.try_get("account_id")?)?,
            project,
            storage,
            command,
        })
    }
}

pub(crate) async fn process(app: &App, job: Job) -> Result<(), Error> {
    match execute(app, &job).await {
        Ok(()) | Err(Error::Conflict("lease_lost")) => Ok(()),
        Err(Error::Conflict("task_state_missing")) => {
            sqlx::query("UPDATE operations SET lease_token=NULL,lease_until=NULL WHERE operation_id=$1 AND task_operation AND status='Queued' AND lease_token=$2")
                .bind(job.id.to_string()).bind(&job.lease).execute(&app.db).await?;
            Ok(())
        }
        Err(error) => {
            // A transient read/SQL failure leaves all task effects rolled back.
            // Retry the retained operation; never hand it to the signing worker.
            let time = now()?;
            tracing::warn!(operation_id = %job.id, code = error.status_code().1, "task execution deferred");
            sqlx::query("UPDATE operations SET attempt_count=LEAST(attempt_count+1,5), next_attempt_at=$3+LEAST(1::bigint << LEAST(attempt_count+1,5)::integer,30), error_code=$4, lease_token=NULL, lease_until=NULL WHERE operation_id=$1 AND task_operation AND status='Queued' AND lease_token=$2 AND lease_until>$3")
                .bind(job.id.to_string()).bind(&job.lease).bind(time).bind(error.status_code().1)
                .execute(&app.db).await?;
            Ok(())
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the task mutation, allocator, result and notifications in one explicit transaction."
)]
async fn execute(app: &App, job: &Job) -> Result<(), Error> {
    let mut tx = app.db.begin().await?;
    let row = sqlx::query("SELECT * FROM operations WHERE operation_id=$1 AND task_operation AND status='Queued' AND lease_token=$2 AND lease_until>$3 FOR UPDATE")
        .bind(job.id.to_string()).bind(&job.lease).bind(now()?).fetch_optional(&mut *tx).await?
        .ok_or(Error::Conflict("lease_lost"))?;
    let expires: i64 = row.try_get("expires_at")?;
    let request = tokio::task::spawn_blocking(move || Request::read(&row)).await??;
    let Some(next) =
        crate::task_freezes::lock_project(&mut tx, request.generation, request.project).await?
    else {
        return Err(Error::Conflict("task_state_missing"));
    };
    match crate::task_freezes::hold(&mut tx, request.generation, request.project).await? {
        crate::task_freezes::Hold::Clear => {}
        crate::task_freezes::Hold::Confirmed(code) => {
            return finish(
                tx,
                job,
                &request,
                ExecutionOutcome::Failed(code.into()),
                None,
            )
            .await;
        }
        crate::task_freezes::Hold::Pending => {
            let time = now()?;
            let changed=sqlx::query("UPDATE operations SET next_attempt_at=$3+1,lease_token=NULL,lease_until=NULL WHERE operation_id=$1 AND task_operation AND status='Queued' AND lease_token=$2 AND lease_until>$3")
                .bind(job.id.to_string()).bind(&job.lease).bind(time).execute(&mut *tx).await?;
            if changed.rows_affected() != 1 {
                return Err(Error::Conflict("lease_lost"));
            }
            tx.commit().await?;
            return Ok(());
        }
    }
    let time = now()?;
    if expires <= time {
        let updated = sqlx::query("UPDATE operations SET status='Expired',error_code='expired',lease_token=NULL,lease_until=NULL WHERE operation_id=$1 AND lease_token=$2 AND lease_until>$3")
            .bind(job.id.to_string()).bind(&job.lease).bind(time).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(Error::Conflict("lease_lost"));
        }
        tx.commit().await?;
        return Ok(());
    }
    let snapshot = app.snapshot().await?;
    if snapshot.info.provider_instance_id != request.generation {
        return Err(Error::Dependency);
    }
    let Some(project) = snapshot
        .projects
        .into_iter()
        .find(|p| p.project_id == request.project)
    else {
        return finish(
            tx,
            job,
            &request,
            ExecutionOutcome::Failed("project_not_found".into()),
            None,
        )
        .await;
    };
    let workers = snapshot.workers;
    let (project, request, permission) = tokio::task::spawn_blocking(move || {
        let known = workers.iter().map(|w| w.account).collect();
        let permission = authorize_task(&project, request.actor, &request.command, &known);
        (project, request, permission)
    })
    .await?;
    if let Err(code) = permission {
        return finish(
            tx,
            job,
            &request,
            ExecutionOutcome::Failed(code.into()),
            None,
        )
        .await;
    }
    let context = project.proposals.iter().find_map(|p| {
        p.milestones
            .iter()
            .find(|m| m.task_storage.task_storage_id == request.storage)
            .map(|m| {
                (
                    p.proposal_id,
                    p.revision,
                    m.milestone_id,
                    m.task_storage.tasks.is_empty(),
                )
            })
    });
    let Some((proposal, revision, milestone, core_empty)) = context else {
        return finish(
            tx,
            job,
            &request,
            ExecutionOutcome::Failed("task_storage_not_found".into()),
            None,
        )
        .await;
    };
    let storage = sqlx::query("SELECT project_id,proposal_id,milestone_id FROM task_storages WHERE provider_instance_id=$1 AND task_storage_id=$2")
        .bind(request.generation.to_string()).bind(request.storage.to_string()).fetch_optional(&mut *tx).await?;
    if storage.is_none() && !core_empty {
        // Only newly created empty core storages may be materialized lazily.
        // Missing preparation must never discard retained provider tasks.
        return Err(Error::Dependency);
    }
    if let Some(row) = &storage
        && (row.try_get::<&str, _>("project_id")? != request.project.to_string()
            || row.try_get::<&str, _>("proposal_id")? != proposal.to_string()
            || row.try_get::<&str, _>("milestone_id")? != milestone.to_string())
    {
        return Err(Error::Dependency);
    }
    let core_command = match &request.command {
        ProviderCommand::CreateTask { .. } | ProviderCommand::DeleteTask { .. } => {
            compact_command(&request.command)
        }
        ProviderCommand::EditTask { task_id, task, .. } => {
            let current = project
                .proposals
                .iter()
                .flat_map(|p| &p.milestones)
                .find(|m| m.task_storage.task_storage_id == request.storage)
                .and_then(|m| m.task_storage.tasks.iter().find(|t| t.task_id == *task_id));
            match current {
                Some(current) if current.assignees != task.assignees => {
                    compact_command(&request.command)
                }
                _ => None,
            }
        }
        _ => None,
    };
    if let Some(command) = core_command {
        // Refuse missing/corrupt preimages before an irreversible core write.
        // Recovery after success must not introduce new content dependencies.
        crate::task_views::current_state(&mut tx, request.generation, next, vec![project]).await?;
        if storage.is_none() {
            sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(request.generation.to_string()).bind(request.storage.to_string()).bind(request.project.to_string()).bind(proposal.to_string()).bind(revision.to_string()).bind(milestone.to_string()).execute(&mut *tx).await?;
        }
        let (json, bytes) = tokio::task::spawn_blocking(move || {
            Ok::<_, Error>((serde_json::to_string(&command)?, command.encode()))
        })
        .await??;
        let converted = sqlx::query("UPDATE operations SET task_operation=FALSE,status='AwaitingSignature',command_json=$3,command_bytes=$4,lease_token=NULL,lease_until=NULL,error_code=NULL,attempt_count=0,next_attempt_at=$5 WHERE operation_id=$1 AND lease_token=$2 AND lease_until>$5 AND task_operation AND status='Queued' AND signable_payload IS NULL AND signed_json IS NULL AND receipt_json IS NULL AND task_result_json IS NULL AND NOT possibly_submitted")
            .bind(request.id.to_string()).bind(&job.lease).bind(json).bind(bytes).bind(now()?).execute(&mut *tx).await?;
        if converted.rows_affected() != 1 {
            return Err(Error::Conflict("lease_lost"));
        }
        tx.commit().await?;
        return Ok(());
    }
    let rows=sqlx::query("SELECT task_id,ordinal,payload,assignees FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=$2 ORDER BY ordinal")
        .bind(request.generation.to_string()).bind(request.storage.to_string()).fetch_all(&mut *tx).await?;
    let (request, changed) = tokio::task::spawn_blocking(move || {
        let changed = match local_task(&project, request.storage, request.actor, &request.command) {
            Err(code) => Err(code),
            Ok(task_id) => {
                let tasks = decode_tasks(rows, next)?;
                if !tasks.iter().any(|task| task.task_id == task_id) {
                    // Core identity exists; missing API content is retryable, not deletion.
                    return Err(Error::Dependency);
                }
                apply(
                    &request.command,
                    tasks,
                    next,
                    UnixSeconds::new(u64::try_from(time).map_err(|_| Error::Internal)?),
                )
                .map(|(tasks, next, kind)| (project, tasks, next, kind))
            }
        };
        Ok::<_, Error>((request, changed))
    })
    .await??;
    let (project, tasks, next, kind) = match changed {
        Ok(value) => value,
        Err(code) => {
            return finish(
                tx,
                job,
                &request,
                ExecutionOutcome::Failed(code.into()),
                None,
            )
            .await;
        }
    };
    let generation = request.generation.to_string();
    if storage.is_none() {
        sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&generation).bind(request.storage.to_string()).bind(request.project.to_string()).bind(proposal.to_string()).bind(revision.to_string()).bind(milestone.to_string()).execute(&mut *tx).await?;
    }
    // Preserve the exact public vector order, including after deletion. The
    // storage-local rewrite also keeps R3b2's ordinal integrity checks valid.
    sqlx::query("DELETE FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=$2")
        .bind(&generation)
        .bind(request.storage.to_string())
        .execute(&mut *tx)
        .await?;
    let prepared = tokio::task::spawn_blocking(move || {
        tasks
            .into_iter()
            .enumerate()
            .map(|(ordinal, task)| {
                let assignees = task
                    .task
                    .assignees
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                Ok::<_, Error>((
                    i64::from(task.task_id),
                    i64::try_from(ordinal).map_err(|_| Error::Dependency)?,
                    serde_json::to_string(&task)?,
                    assignees,
                ))
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .await??;
    for (id, ordinal, payload, assignees) in &prepared {
        sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(&generation).bind(id).bind(request.storage.to_string()).bind(ordinal).bind(payload).bind(assignees).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE task_storages SET proposal_revision=$3 WHERE provider_instance_id=$1 AND task_storage_id=$2")
        .bind(&generation).bind(request.storage.to_string()).bind(revision.to_string()).execute(&mut *tx).await?;
    sqlx::query("UPDATE task_generations SET next_task_id=$2 WHERE provider_instance_id=$1")
        .bind(&generation)
        .bind(i64::from(next))
        .execute(&mut *tx)
        .await?;
    let mut recipients = BTreeSet::from([project.client, project.coordinator]);
    recipients.extend(
        project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.assignments)
            .map(|a| a.worker),
    );
    recipients.extend(
        project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.task_storage.tasks)
            .flat_map(|t| &t.assignees)
            .copied(),
    );
    let event = AdapterTaskEvent {
        source: TaskEventSource::AdapterTask,
        provider_instance_id: request.generation,
        operation_id: request.id,
        kind,
        project_id: request.project,
        task_storage_id: request.storage,
        recipients: recipients.into_iter().collect(),
        occurred_at: UnixSeconds::new(u64::try_from(time).map_err(|_| Error::Internal)?),
        origin: request.actor,
    };
    finish(tx, job, &request, ExecutionOutcome::Success, Some(event)).await
}

pub(crate) fn decode_tasks(rows: Vec<PgRow>, next: u32) -> Result<Vec<TaskView>, Error> {
    rows.into_iter()
        .enumerate()
        .map(|(ordinal, row)| {
            let task: TaskView = serde_json::from_str(row.try_get("payload")?)?;
            task.task.validate().map_err(|_| Error::Dependency)?;
            if task.task_id == 0
                || task.task_id >= next
                || i64::from(task.task_id) != row.try_get::<i64, _>("task_id")?
                || i64::try_from(ordinal).map_err(|_| Error::Dependency)?
                    != row.try_get::<i64, _>("ordinal")?
                || task
                    .task
                    .assignees
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    != row.try_get::<Vec<String>, _>("assignees")?
            {
                return Err(Error::Dependency);
            }
            Ok(task)
        })
        .collect()
}

fn authorize_task(
    project: &ProjectCore,
    actor: AccountId32,
    command: &ProviderCommand,
    workers: &BTreeSet<AccountId32>,
) -> Result<(), &'static str> {
    if project.cancelled {
        return Err("project_cancelled");
    }
    if project.active_dispute_id.is_some() {
        return Err("project_disputed");
    }
    let coordinator = || {
        if project.coordinator == actor {
            Ok(())
        } else {
            Err("coordinator_required")
        }
    };
    let known = |task: &generated_contracts::TaskDefinition| {
        if task.assignees.iter().all(|a| workers.contains(a)) {
            Ok(())
        } else {
            Err("unknown_task_assignee")
        }
    };
    match command {
        ProviderCommand::CreateTask { task, .. } | ProviderCommand::EditTask { task, .. } => {
            coordinator()?;
            known(task)?;
        }
        ProviderCommand::DeleteTask { .. } => coordinator()?,
        ProviderCommand::UpdateTaskProgress { .. } => {
            if actor == project.client {
                return Err("client_read_only");
            }
        }
        _ => return Err("invalid_project_message"),
    }
    Ok(())
}

fn local_task(
    project: &ProjectCore,
    storage: EntityId,
    actor: AccountId32,
    command: &ProviderCommand,
) -> Result<u32, &'static str> {
    let task_id = match command {
        ProviderCommand::EditTask { task_id, .. }
        | ProviderCommand::UpdateTaskProgress { task_id, .. } => *task_id,
        _ => return Err("invalid_project_message"),
    };
    let member = project
        .proposals
        .iter()
        .flat_map(|p| &p.milestones)
        .find(|m| m.task_storage.task_storage_id == storage)
        .and_then(|m| m.task_storage.tasks.iter().find(|t| t.task_id == task_id))
        .ok_or("task_not_found")?;
    match command {
        ProviderCommand::EditTask { task, .. } if member.assignees != task.assignees => {
            return Err("task_membership_requires_core");
        }
        ProviderCommand::UpdateTaskProgress { .. }
            if project.coordinator != actor && !member.assignees.contains(&actor) =>
        {
            return Err("task_assignee_required");
        }
        _ => (),
    }
    Ok(task_id)
}

fn apply(
    command: &ProviderCommand,
    mut tasks: Vec<TaskView>,
    next: u32,
    time: UnixSeconds,
) -> Result<(Vec<TaskView>, u32, TaskEventKind), &'static str> {
    let kind = match command {
        ProviderCommand::EditTask { task_id, task, .. } => {
            let current = tasks
                .iter_mut()
                .find(|t| t.task_id == *task_id)
                .ok_or("task_not_found")?;
            current.task = task.clone();
            current.updated_at = time;
            TaskEventKind::Updated
        }
        ProviderCommand::UpdateTaskProgress {
            task_id, progress, ..
        } => {
            let current = tasks
                .iter_mut()
                .find(|t| t.task_id == *task_id)
                .ok_or("task_not_found")?;
            current.task.status = progress.status;
            current.task.logged_minutes = progress.logged_minutes;
            current.updated_at = time;
            TaskEventKind::Updated
        }
        _ => return Err("invalid_project_message"),
    };
    Ok((tasks, next, kind))
}

async fn finish(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    job: &Job,
    request: &Request,
    outcome: ExecutionOutcome,
    event: Option<AdapterTaskEvent>,
) -> Result<(), Error> {
    let time = now()?;
    let finalized_at = event.as_ref().map_or(
        UnixSeconds::new(u64::try_from(time).map_err(|_| Error::Internal)?),
        |e| e.occurred_at,
    );
    let result = TaskOperationResult {
        outcome,
        task_storage_id: request.storage,
        finalized_at,
    };
    let result_json = serde_json::to_string(&result)?;
    let changed=sqlx::query("UPDATE operations SET status='Finalized',task_result_json=$4,error_code=NULL,lease_token=NULL,lease_until=NULL WHERE operation_id=$1 AND task_operation AND status='Queued' AND lease_token=$2 AND lease_until>$3")
        .bind(job.id.to_string()).bind(&job.lease).bind(time).bind(result_json).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::Conflict("lease_lost"));
    }
    if let Some(event) = event {
        let json = serde_json::to_string(&event)?;
        sqlx::query("SELECT pg_advisory_xact_lock(621008)")
            .execute(&mut *tx)
            .await?;
        for recipient in &event.recipients {
            sqlx::query("INSERT INTO notifications(account_id,provider_instance_id,api_operation_id,event_json) VALUES($1,$2,$3,$4)")
                .bind(recipient.to_string()).bind(request.generation.to_string()).bind(job.id.to_string()).bind(&json).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO audit_records(operation_id,event_kind,result_code,occurred_at) VALUES($1,'task_finalized',$2,$3)")
        .bind(job.id.to_string()).bind(match &result.outcome {ExecutionOutcome::Success=>"ok",ExecutionOutcome::Failed(code)=>code}).bind(time).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
