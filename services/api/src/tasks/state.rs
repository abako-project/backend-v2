//! Initialize API task content once for a new, empty provider generation.
//! Restart never invents missing content for an existing core task graph.
use crate::state::{App, Error};

pub(crate) async fn initialize_empty(app: &App) -> Result<(), Error> {
    let state: protocol::provider_core::TaskState = app
        .get(&app.config.provider_url, "/internal/task-state")
        .await?;
    let state = tokio::task::spawn_blocking(move || {
        state.validate().map_err(|_| Error::Dependency)?;
        Ok::<_, Error>(state)
    })
    .await??;
    let generation = state.generation;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(621009)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(621007)")
        .execute(&mut *tx)
        .await?;
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT next_task_id FROM task_generations WHERE provider_instance_id=$1 FOR UPDATE",
    )
    .bind(generation.to_string())
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_some() {
        tx.commit().await?;
        return Ok(());
    }
    if state.next_task_id != 1
        || state.has_task_history
        || state
            .storages
            .iter()
            .any(|record| !record.storage.tasks.is_empty())
    {
        return Err(Error::Conflict("task_state_missing"));
    }
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE status NOT IN ('Finalized','Rejected','Expired') OR task_operation OR task_project_id IS NOT NULL OR (possibly_submitted AND status IN ('Rejected','Expired') AND NOT(status='Rejected' AND error_code IS NOT DISTINCT FROM 'provider_rejected'))) OR EXISTS(SELECT 1 FROM task_freezes)")
        .fetch_one(&mut *tx).await?;
    if pending {
        return Err(Error::Conflict("task_state_missing"));
    }
    let snapshot = app.snapshot().await?;
    if snapshot.info.provider_instance_id != generation
        || snapshot
            .projects
            .iter()
            .any(|project| project.active_dispute_id.is_some())
        || !state.storages.is_empty()
    {
        return Err(Error::Conflict("task_state_missing"));
    }
    sqlx::query("INSERT INTO task_generations(provider_instance_id,next_task_id) VALUES($1,1)")
        .bind(generation.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
