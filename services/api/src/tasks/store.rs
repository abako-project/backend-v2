//! Task row encoding and writes inside the caller's database transaction.
use crate::state::Error;
use protocol::{EntityId, ProviderInstanceId, TaskView};
use sqlx::{PgConnection, Row, postgres::PgRow};

pub(super) type TaskRow = (i64, i64, String, Vec<String>);

pub(super) fn encode(tasks: Vec<TaskView>) -> Result<Vec<TaskRow>, Error> {
    tasks
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
        .collect()
}

/// Preserve public vector order, including after deletion. Both local edits and
/// confirmed membership operations use the same row representation.
pub(super) async fn replace(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    storage: EntityId,
    rows: Vec<TaskRow>,
) -> Result<(), Error> {
    sqlx::query("DELETE FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=$2")
        .bind(generation.to_string())
        .bind(storage.to_string())
        .execute(&mut *connection)
        .await?;
    for (id, ordinal, payload, assignees) in rows {
        sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(generation.to_string()).bind(id).bind(storage.to_string()).bind(ordinal).bind(payload).bind(assignees).execute(&mut *connection).await?;
    }
    Ok(())
}

pub(super) fn decode(rows: Vec<PgRow>, next: u32) -> Result<Vec<TaskView>, Error> {
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
