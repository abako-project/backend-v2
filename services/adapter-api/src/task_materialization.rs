//! Materialize immutable API content only after the exact core intent succeeds.
use crate::state::{App, Error, parse};
use generated_contracts::{
    ExecutionOutcome, OperationId, OperationReceipt, ProviderCommand, TaskView,
    provider_core::TaskAllocation,
};
use sqlx::{PgConnection, Row};

/// Resolve private allocation before opening the finality transaction. A lost
/// response remains an uncertain operation and retains the project FIFO barrier.
pub(crate) async fn allocation(
    app: &App,
    command: &ProviderCommand,
    receipt_json: &str,
) -> Result<Option<TaskAllocation>, Error> {
    let receipt: OperationReceipt = serde_json::from_str(receipt_json)?;
    if receipt.outcome != ExecutionOutcome::Success {
        return Ok(None);
    }
    let ProviderCommand::CreateTaskCore {
        project_id,
        task_storage_id,
        ..
    } = command
    else {
        return Ok(None);
    };
    let allocated: TaskAllocation = app
        .get(
            &app.config.provider_url,
            &format!("/internal/task-allocations/{}", receipt.operation_id),
        )
        .await?;
    if allocated.receipt != receipt
        || allocated.project_id != *project_id
        || allocated.task_storage_id != *task_storage_id
        || allocated.task_id == 0
        || allocated.task_id == u32::MAX
        || receipt.created_entity_id != Some(*task_storage_id)
    {
        return Err(Error::Dependency);
    }
    Ok(Some(allocated))
}

/// The caller holds the fenced operation row. No later expiry, permission or
/// freeze check can undo a successful core execution; this transaction only
/// applies its previously accepted immutable content and commits real finality.
pub(crate) async fn complete(
    connection: &mut PgConnection,
    operation: OperationId,
    allocated: Option<&TaskAllocation>,
) -> Result<(), Error> {
    let row = sqlx::query("SELECT * FROM operations WHERE operation_id=$1 AND NOT task_operation AND status='Finalized'")
        .bind(operation.to_string()).fetch_optional(&mut *connection).await?.ok_or(Error::Dependency)?;
    let (original, receipt) = tokio::task::spawn_blocking(move || {
        let command: ProviderCommand = serde_json::from_str(row.try_get("command_json")?)?;
        let original =
            crate::task_execution::provider_intent(&row, &command)?.ok_or(Error::Dependency)?;
        let receipt: OperationReceipt = serde_json::from_str(row.try_get("receipt_json")?)?;
        if receipt.operation_id != operation
            || receipt.origin != parse(row.try_get("account_id")?)?
            || Some(receipt.provider_instance_id.to_string().as_str())
                != row.try_get::<Option<&str>, _>("provider_instance_id")?
        {
            return Err(Error::Dependency);
        }
        Ok::<_, Error>((original, receipt))
    })
    .await??;
    if receipt.outcome != ExecutionOutcome::Success {
        return Ok(());
    }
    let project = original.project_id().ok_or(Error::Dependency)?;
    let generation = receipt.provider_instance_id;
    let next = crate::task_freezes::lock_project(connection, generation, project)
        .await?
        .ok_or(Error::Dependency)?;
    let storage = match &original {
        ProviderCommand::CreateTask {
            task_storage_id, ..
        }
        | ProviderCommand::EditTask {
            task_storage_id, ..
        }
        | ProviderCommand::DeleteTask {
            task_storage_id, ..
        } => *task_storage_id,
        _ => return Err(Error::Dependency),
    };
    if receipt.created_entity_id != Some(storage) {
        return Err(Error::Dependency);
    }
    let context: Option<String> = sqlx::query_scalar(
        "SELECT project_id FROM task_storages WHERE provider_instance_id=$1 AND task_storage_id=$2",
    )
    .bind(generation.to_string())
    .bind(storage.to_string())
    .fetch_optional(&mut *connection)
    .await?;
    if context.as_deref() != Some(project.to_string().as_str()) {
        return Err(Error::Dependency);
    }
    let rows = sqlx::query("SELECT task_id,ordinal,payload,assignees FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=$2 ORDER BY ordinal")
        .bind(generation.to_string()).bind(storage.to_string()).fetch_all(&mut *connection).await?;
    let allocated = allocated.cloned();
    let Prepared {
        rows: prepared,
        next,
    } = tokio::task::spawn_blocking(move || {
        prepare(original, &receipt, allocated.as_ref(), rows, next)
    })
    .await??;
    sqlx::query("DELETE FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=$2")
        .bind(generation.to_string())
        .bind(storage.to_string())
        .execute(&mut *connection)
        .await?;
    for (id, ordinal, payload, assignees) in prepared {
        sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(generation.to_string()).bind(id).bind(storage.to_string()).bind(ordinal).bind(payload).bind(assignees).execute(&mut *connection).await?;
    }
    sqlx::query("UPDATE task_generations SET next_task_id=GREATEST(next_task_id,$2) WHERE provider_instance_id=$1")
        .bind(generation.to_string()).bind(i64::from(next)).execute(connection).await?;
    Ok(())
}

struct Prepared {
    rows: Vec<(i64, i64, String, Vec<String>)>,
    next: u32,
}

fn prepare(
    original: ProviderCommand,
    receipt: &OperationReceipt,
    allocated: Option<&TaskAllocation>,
    rows: Vec<sqlx::postgres::PgRow>,
    next: u32,
) -> Result<Prepared, Error> {
    let project = original.project_id().ok_or(Error::Dependency)?;
    let storage = receipt.created_entity_id.ok_or(Error::Dependency)?;
    let mut tasks = crate::task_execution::decode_tasks(rows, next)?;
    let mut next = next;
    match original {
        ProviderCommand::CreateTask { task, .. } => {
            let allocation = allocated.ok_or(Error::Dependency)?;
            if &allocation.receipt != receipt
                || allocation.project_id != project
                || allocation.task_storage_id != storage
                || allocation.task_id == 0
                || allocation.task_id == u32::MAX
                || tasks.iter().any(|t| t.task_id == allocation.task_id)
            {
                return Err(Error::Dependency);
            }
            next = next.max(allocation.task_id.checked_add(1).ok_or(Error::Dependency)?);
            tasks.push(TaskView {
                task_id: allocation.task_id,
                reporter: receipt.origin,
                created_at: receipt.finalized_at,
                updated_at: receipt.finalized_at,
                task,
            });
        }
        ProviderCommand::EditTask { task_id, task, .. } => {
            if allocated.is_some() {
                return Err(Error::Dependency);
            }
            let current = tasks
                .iter_mut()
                .find(|t| t.task_id == task_id)
                .ok_or(Error::Dependency)?;
            current.task = task;
            current.updated_at = receipt.finalized_at;
        }
        ProviderCommand::DeleteTask { task_id, .. } => {
            if allocated.is_some() || !tasks.iter().any(|t| t.task_id == task_id) {
                return Err(Error::Dependency);
            }
            tasks.retain(|t| t.task_id != task_id);
        }
        _ => return Err(Error::Dependency),
    }
    let prepared = tasks
        .into_iter()
        .enumerate()
        .map(|(ordinal, task)| {
            Ok::<_, Error>((
                i64::from(task.task_id),
                i64::try_from(ordinal).map_err(|_| Error::Dependency)?,
                serde_json::to_string(&task)?,
                task.task
                    .assignees
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Prepared {
        rows: prepared,
        next,
    })
}
