//! Task presentation and membership. Financial/lifecycle callers keep raw core reads.
use crate::state::{App, Error, parse};
use protocol::{
    AccountId32, EntityId, ProviderInstanceId,
    provider_core::{ProjectCore, TaskSnapshot, TaskSnapshotRecord},
};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::BTreeMap;

pub(crate) async fn counter(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
) -> Result<Option<u32>, Error> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT next_task_id FROM task_generations WHERE provider_instance_id=$1",
    )
    .bind(generation.to_string())
    .fetch_optional(connection)
    .await?;
    value
        .map(|n| u32::try_from(n).map_err(|_| Error::Dependency))
        .transpose()
}
fn ids(projects: &[ProjectCore]) -> Vec<String> {
    projects
        .iter()
        .flat_map(|p| &p.proposals)
        .flat_map(|p| &p.milestones)
        .map(|m| m.task_storage.task_storage_id.to_string())
        .collect()
}

/// Only contract membership grants visibility, independently of API content availability.
pub(crate) fn visible(projects: Vec<ProjectCore>, account: AccountId32) -> Vec<ProjectCore> {
    projects
        .into_iter()
        .filter(|p| crate::operations::visible(p, account))
        .collect()
}
pub(crate) fn project(project: ProjectCore, account: AccountId32) -> Option<ProjectCore> {
    crate::operations::visible(&project, account).then_some(project)
}

async fn read(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    next: u32,
    projects: Vec<ProjectCore>,
) -> Result<TaskSnapshot, Error> {
    let storage_ids = ids(&projects);
    let contexts=sqlx::query("SELECT task_storage_id,project_id,proposal_id,milestone_id FROM task_storages WHERE provider_instance_id=$1 AND task_storage_id=ANY($2)")
        .bind(generation.to_string()).bind(&storage_ids).fetch_all(&mut *connection).await?;
    let rows=sqlx::query("SELECT task_id,task_storage_id,ordinal,payload,assignees FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=ANY($2) ORDER BY task_storage_id,ordinal")
        .bind(generation.to_string()).bind(&storage_ids).fetch_all(connection).await?;
    tokio::task::spawn_blocking(move || {
        apply(generation, next, projects, contexts, rows).map_err(|_| Error::Dependency)
    })
    .await?
}
fn apply(
    generation: ProviderInstanceId,
    next: u32,
    projects: Vec<ProjectCore>,
    contexts: Vec<PgRow>,
    rows: Vec<PgRow>,
) -> Result<TaskSnapshot, Error> {
    let mut stored = BTreeMap::new();
    for row in contexts {
        stored.insert(
            parse::<EntityId>(row.try_get("task_storage_id")?)?,
            (
                parse::<EntityId>(row.try_get("project_id")?)?,
                parse::<EntityId>(row.try_get("proposal_id")?)?,
                parse::<EntityId>(row.try_get("milestone_id")?)?,
            ),
        );
    }
    let mut tasks = BTreeMap::<EntityId, Vec<PgRow>>::new();
    for row in rows {
        tasks
            .entry(parse(row.try_get("task_storage_id")?)?)
            .or_default()
            .push(row);
    }
    let mut validated = Vec::new();
    for project in projects {
        for proposal in project.proposals {
            for milestone in proposal.milestones {
                let storage = milestone.task_storage;
                if storage.milestone_id != milestone.milestone_id {
                    return Err(Error::Dependency);
                }
                match stored.remove(&storage.task_storage_id) {
                    Some(context)
                        if context
                            == (
                                project.project_id,
                                proposal.proposal_id,
                                milestone.milestone_id,
                            ) => {}
                    None if storage.tasks.is_empty() => {}
                    _ => return Err(Error::Dependency),
                }
                let content = crate::task_execution::decode_tasks(
                    tasks.remove(&storage.task_storage_id).unwrap_or_default(),
                    next,
                )
                .map_err(|_| Error::Dependency)?;
                let mut content: BTreeMap<_, _> = content
                    .into_iter()
                    .map(|task| (task.task_id, task))
                    .collect();
                let mut composed = Vec::with_capacity(storage.tasks.len());
                for core in &storage.tasks {
                    let mut task = content.remove(&core.task_id).ok_or(Error::Dependency)?;
                    task.task.assignees.clone_from(&core.assignees);
                    composed.push(task);
                }
                validated.push(TaskSnapshotRecord {
                    project_id: project.project_id,
                    proposal_id: proposal.proposal_id,
                    proposal_revision: proposal.revision,
                    storage: protocol::TaskStorageView {
                        task_storage_id: storage.task_storage_id,
                        milestone_id: storage.milestone_id,
                        tasks: composed,
                    },
                });
            }
        }
    }
    if !stored.is_empty() || !tasks.is_empty() {
        return Err(Error::Dependency);
    }
    validated.sort_by_key(|record| record.storage.task_storage_id);
    let state = TaskSnapshot {
        schema_version: 1,
        generation,
        next_task_id: next,
        storages: validated,
    };
    state.validate().map_err(|_| Error::Dependency)?;
    Ok(state)
}

/// Compose only an already-authorized project's content in one database snapshot.
pub(crate) async fn content(
    app: &App,
    generation: ProviderInstanceId,
    project: ProjectCore,
) -> Result<TaskSnapshot, Error> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let next = counter(&mut tx, generation)
        .await?
        .ok_or(Error::Dependency)?;
    let tasks = read(&mut tx, generation, next, vec![project]).await?;
    tx.commit().await?;
    Ok(tasks)
}

/// Read current content within the caller's transaction before a core task write.
pub(crate) async fn current_state(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    next: u32,
    projects: Vec<ProjectCore>,
) -> Result<TaskSnapshot, Error> {
    read(connection, generation, next, projects).await
}
