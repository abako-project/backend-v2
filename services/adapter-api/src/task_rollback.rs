//! Paused owner-only task rollback. No forward activation or cross-store writes.
use crate::state::{App, Error};
use generated_contracts::{
    ProviderInstanceId,
    provider_core::{CoreSnapshotV4, TaskStateV1},
};
use sqlx::{PgConnection, Row};
use std::{fs::File, io::Read, path::PathBuf};

async fn paused(connection: &mut PgConnection) -> Result<(), Error> {
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
    app: &App,
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
) -> Result<(TaskStateV1, CoreSnapshotV4, bool), Error> {
    let row=sqlx::query("SELECT next_task_id,api_owned FROM task_generations WHERE provider_instance_id=$1 FOR NO KEY UPDATE")
        .bind(generation.to_string()).fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
    let owned = row.try_get("api_owned")?;
    let next =
        u32::try_from(row.try_get::<i64, _>("next_task_id")?).map_err(|_| Error::Dependency)?;
    let snapshot = app.snapshot().await?;
    if snapshot.info.provider_instance_id != generation {
        return Err(Error::Conflict("provider_instance_changed"));
    }
    let projects = tokio::task::spawn_blocking(move || {
        let projects = snapshot.projects.clone();
        (snapshot, projects)
    })
    .await?;
    let (snapshot, projects) = projects;
    let tasks = crate::task_views::current_state(connection, generation, next, projects).await?;
    Ok((tasks, snapshot, owned))
}

pub(crate) async fn export(
    app: &App,
    generation: ProviderInstanceId,
    path: PathBuf,
) -> Result<(), Error> {
    let mut tx = app.db.begin().await?;
    paused(&mut tx).await?;
    let (state, _, owned) = current(app, &mut tx, generation).await?;
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
    let (current, snapshot, _owned) = current(app, &mut tx, saved.generation).await?;
    let response = app
        .internal(
            reqwest::Method::GET,
            &app.config.provider_url,
            "/internal/task-state",
        )
        .send()
        .await
        .map_err(|_| Error::Dependency)?;
    let bytes = crate::state::bounded_response(response).await?;
    let generation = saved.generation;
    let next = current.next_task_id;
    tokio::task::spawn_blocking(move || {
        let target: TaskStateV1 = serde_json::from_slice(&bytes)?;
        target.validate().map_err(|_| Error::Dependency)?;
        if target != saved || current != saved {
            return Err(Error::Conflict("task_rollback_state_mismatch"));
        }
        Ok::<_, Error>(())
    })
    .await??;
    for project in snapshot.projects {
        if let Some(id) = project.active_dispute_id {
            let case = crate::disputes::read(app, id).await?;
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
            crate::task_freezes::verify_restored_case(
                &mut tx,
                generation,
                next,
                proposal.proposal_id,
                case,
            )
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
