//! Paused owner-only task rollback. No forward activation or cross-store writes.
use crate::state::{App, Error};
use generated_contracts::{
    ProviderInstanceId,
    provider_core::{CoreSnapshotV4, ProjectCore, TaskStateV1, TaskStateV2},
};
use sqlx::{PgConnection, Row};
use std::{fs::File, io::Read, path::PathBuf};

pub(crate) async fn paused(connection: &mut PgConnection) -> Result<(), Error> {
    // Match preparation's lock order. Acquire before reading the queue: a
    // repeatable-read snapshot taken while waiting could miss a just-admitted job.
    sqlx::query("SELECT pg_advisory_xact_lock(621009)")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(621007)")
        .execute(&mut *connection)
        .await?;
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE status NOT IN ('Finalized','Rejected','Expired') OR (NOT task_operation AND status='Finalized' AND receipt_json IS NULL) OR (possibly_submitted AND status IN ('Rejected','Expired') AND NOT(status='Rejected' AND error_code IS NOT DISTINCT FROM 'provider_rejected'))) OR EXISTS(SELECT 1 FROM task_freezes WHERE state='Pending')")
        .fetch_one(connection).await?;
    if pending {
        return Err(Error::Conflict("task_rollback_operations_pending"));
    }
    Ok(())
}
async fn current(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    next: u32,
    projects: Vec<ProjectCore>,
) -> Result<(TaskStateV1, bool), Error> {
    let row=sqlx::query("SELECT next_task_id,api_owned,storage_count,task_count FROM task_generations WHERE provider_instance_id=$1 FOR NO KEY UPDATE")
        .bind(generation.to_string()).fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
    // Counts describe all retained API rows, including obsolete core contexts.
    // Check before selecting live targets so a missing row cannot become an
    // apparently valid empty export and then overwrite the provider's tasks.
    let (storage_count,task_count):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM task_storages WHERE provider_instance_id=$1),(SELECT count(*) FROM task_items WHERE provider_instance_id=$1)")
        .bind(generation.to_string()).fetch_one(&mut *connection).await?;
    if storage_count != row.try_get::<i64, _>("storage_count")?
        || task_count != row.try_get::<i64, _>("task_count")?
    {
        return Err(Error::Dependency);
    }
    let owned = row.try_get("api_owned")?;
    let stored_next =
        u32::try_from(row.try_get::<i64, _>("next_task_id")?).map_err(|_| Error::Dependency)?;
    if stored_next > next {
        return Err(Error::Dependency);
    }
    let tasks = crate::task_views::current_state(connection, generation, next, projects).await?;
    Ok((tasks, owned))
}

pub(crate) async fn export(
    app: &App,
    generation: ProviderInstanceId,
    path: PathBuf,
) -> Result<(), Error> {
    let mut tx = app.db.begin().await?;
    paused(&mut tx).await?;
    let core: TaskStateV2 = fetch_target(app, "/internal/task-state").await?;
    let snapshot = app.snapshot().await?;
    if core.generation != generation || snapshot.info.provider_instance_id != generation {
        return Err(Error::Conflict("provider_instance_changed"));
    }
    let (state, owned) = current(&mut tx, generation, core.next_task_id, snapshot.projects).await?;
    let state = tokio::task::spawn_blocking(move || {
        verify_core(&state, &core)?;
        Ok::<_, Error>(state)
    })
    .await??;
    if !owned {
        return Err(Error::Conflict("task_rollback_not_api_owned"));
    }
    // Keep admission fenced through file creation; this is a projection, not a
    // full PostgreSQL/provider/custody backup or proof of future source freshness.
    crate::task_state::write(state, path).await?;
    tx.commit().await?;
    Ok(())
}
async fn read(path: PathBuf) -> Result<TaskStateV1, Error> {
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        File::open(path)?.take(33_554_433).read_to_end(&mut bytes)?;
        if bytes.len() > 33_554_432 {
            return Err(Error::Invalid);
        }
        let state: TaskStateV1 = serde_json::from_slice(&bytes)?;
        state.validate().map_err(|_| Error::Invalid)?;
        Ok(state)
    })
    .await?
}

pub(crate) async fn complete(app: &App, path: PathBuf) -> Result<(), Error> {
    let saved = read(path).await?;
    let mut tx = app.db.begin().await?;
    paused(&mut tx).await?;
    // This endpoint now belongs to the explicitly restored predecessor. Decode
    // only its V4 representation here, never as a live-provider fallback.
    let snapshot: CoreSnapshotV4 = fetch_target(app, "/internal/snapshot").await?;
    if snapshot.schema_version != 4 || snapshot.info.provider_instance_id != saved.generation {
        return Err(Error::Conflict("provider_instance_changed"));
    }
    let target: TaskStateV1 = fetch_target(app, "/internal/task-state").await?;
    let generation = saved.generation;
    let (snapshot, projects) = tokio::task::spawn_blocking(move || {
        let projects = snapshot.projects.iter().cloned().map(Into::into).collect();
        (snapshot, projects)
    })
    .await?;
    let (current, _owned) = current(&mut tx, generation, target.next_task_id, projects).await?;
    let next = current.next_task_id;
    tokio::task::spawn_blocking(move || {
        target.validate().map_err(|_| Error::Dependency)?;
        if target != saved || current != saved {
            return Err(Error::Conflict("task_rollback_state_mismatch"));
        }
        Ok::<_, Error>(())
    })
    .await??;
    crate::task_migration::verify_inventory(
        &mut tx,
        generation,
        snapshot
            .projects
            .iter()
            .filter_map(|p| p.active_dispute_id)
            .collect(),
    )
    .await?;
    for project in snapshot.projects {
        if let Some(id) = project.active_dispute_id {
            let case: generated_contracts::provider_core::CoreDisputeViewV4 =
                fetch_target(app, &format!("/internal/disputes/{id}")).await?;
            let (proposal, case) = tokio::task::spawn_blocking(move || {
                let proposal = project
                    .proposals
                    .iter()
                    .find(|p| {
                        p.milestones
                            .iter()
                            .any(|m| m.milestone_id == case.milestone.milestone_id)
                    })
                    .ok_or(Error::Dependency)?;
                let milestone = proposal
                    .milestones
                    .iter()
                    .find(|m| m.milestone_id == case.milestone.milestone_id)
                    .ok_or(Error::Dependency)?;
                if case.dispute.project_id != project.project_id
                    || case.dispute.dispute_id != id
                    || *milestone != case.milestone
                    || proposal.revision != case.dispute.proposal_revision
                {
                    return Err(Error::Dependency);
                }
                Ok::<_, Error>((proposal.proposal_id, case))
            })
            .await??;
            crate::task_freezes::verify_restored_case(&mut tx, generation, next, proposal, case)
                .await?;
        }
    }
    sqlx::query("UPDATE task_generations SET api_owned=FALSE WHERE provider_instance_id=$1")
        .bind(generation.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Both reads occur while the owner has paused core writes. An actual allocator
/// (including deleted gaps) must accompany the exact selected core graph.
pub(crate) fn verify_core(tasks: &TaskStateV1, core: &TaskStateV2) -> Result<(), Error> {
    core.validate().map_err(|_| Error::Dependency)?;
    if tasks.generation != core.generation
        || tasks.next_task_id != core.next_task_id
        || tasks.storages.len() != core.storages.len()
    {
        return Err(Error::Dependency);
    }
    for record in &tasks.storages {
        let selected = core
            .storages
            .iter()
            .find(|r| r.storage.task_storage_id == record.storage.task_storage_id)
            .ok_or(Error::Dependency)?;
        if selected.project_id != record.project_id
            || selected.proposal_id != record.proposal_id
            || selected.proposal_revision != record.proposal_revision
            || selected.storage
                != generated_contracts::provider_core::TaskStorageCore::from(&record.storage)
        {
            return Err(Error::Dependency);
        }
    }
    Ok(())
}

/// Owner exchanges can carry large task narratives. Bound transport bytes, then
/// decode on the blocking pool rather than delaying the async service reactor.
pub(crate) async fn fetch_target<T: serde::de::DeserializeOwned + Send + 'static>(
    app: &App,
    path: &str,
) -> Result<T, Error> {
    let response = app
        .internal(reqwest::Method::GET, &app.config.provider_url, path)
        .send()
        .await
        .map_err(|_| Error::Dependency)?;
    let bytes = crate::state::bounded_response(response).await?;
    tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes).map_err(Error::from)).await?
}
