//! Task presentation and membership. Financial/lifecycle callers keep raw core reads.
use crate::state::{App, Error, parse};
use generated_contracts::{
    AccountId32, EntityId, ProviderInstanceId,
    provider_core::{ProjectCore, TaskStateV1, TaskStorageRecordV1},
};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) async fn owned(app: &App, generation: ProviderInstanceId) -> Result<bool, Error> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT api_owned FROM task_generations WHERE provider_instance_id=$1",
    )
    .bind(generation.to_string())
    .fetch_optional(&app.db)
    .await?
    .unwrap_or(false))
}
pub(crate) async fn counter(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
) -> Result<Option<u32>, Error> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT next_task_id FROM task_generations WHERE provider_instance_id=$1 AND api_owned",
    )
    .bind(generation.to_string())
    .fetch_optional(connection)
    .await?;
    value
        .map(|n| u32::try_from(n).map_err(|_| Error::Dependency))
        .transpose()
}
fn contractual_access(project: &ProjectCore, account: AccountId32) -> bool {
    project.client == account
        || project.coordinator == account
        || project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.assignments)
            .any(|a| a.worker == account)
}
fn ids(projects: &[ProjectCore]) -> Vec<String> {
    projects
        .iter()
        .flat_map(|p| &p.proposals)
        .flat_map(|p| &p.milestones)
        .map(|m| m.task_storage.task_storage_id.to_string())
        .collect()
}

/// One coherent PG graph, selected before task-based visibility. The projection
/// array only selects candidates; validated typed tasks establish actual access.
async fn selected(
    app: &App,
    generation: ProviderInstanceId,
    projects: Vec<ProjectCore>,
    account: AccountId32,
) -> Result<(Vec<ProjectCore>, bool), Error> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let Some(next) = counter(&mut tx, generation).await? else {
        tx.commit().await?;
        return Ok((
            projects
                .into_iter()
                .filter(|p| crate::operations::visible(p, account))
                .collect(),
            false,
        ));
    };
    let candidates:Vec<String>=sqlx::query_scalar("SELECT DISTINCT task_storage_id FROM task_items WHERE provider_instance_id=$1 AND task_storage_id=ANY($2) AND assignees @> ARRAY[$3]::text[]")
        .bind(generation.to_string()).bind(ids(&projects)).bind(account.to_string()).fetch_all(&mut *tx).await?;
    let candidates: BTreeSet<_> = candidates.into_iter().collect();
    let selected = projects
        .into_iter()
        .filter(|p| {
            contractual_access(p, account)
                || p.proposals
                    .iter()
                    .flat_map(|p| &p.milestones)
                    .any(|m| candidates.contains(&m.task_storage.task_storage_id.to_string()))
        })
        .collect();
    let projects = read(&mut tx, generation, next, selected).await?;
    tx.commit().await?;
    Ok((
        projects
            .into_iter()
            .filter(|p| crate::operations::visible(p, account))
            .collect(),
        true,
    ))
}

pub(crate) async fn visible(
    app: &App,
    generation: ProviderInstanceId,
    projects: Vec<ProjectCore>,
    account: AccountId32,
) -> Result<Vec<ProjectCore>, Error> {
    Ok(selected(app, generation, projects, account).await?.0)
}

/// Reads only the selected, visible project. Candidate projection rows never
/// establish access until their typed task payloads have also been validated.
pub(crate) async fn project(
    app: &App,
    generation: ProviderInstanceId,
    project: ProjectCore,
    account: AccountId32,
) -> Result<(Option<ProjectCore>, bool), Error> {
    let (mut projects, owned) = selected(app, generation, vec![project], account).await?;
    Ok((projects.pop(), owned))
}

async fn read(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    next: u32,
    projects: Vec<ProjectCore>,
) -> Result<Vec<ProjectCore>, Error> {
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
    mut projects: Vec<ProjectCore>,
    contexts: Vec<PgRow>,
    rows: Vec<PgRow>,
) -> Result<Vec<ProjectCore>, Error> {
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
    for project in &mut projects {
        for proposal in &mut project.proposals {
            for milestone in &mut proposal.milestones {
                let storage = &mut milestone.task_storage;
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
                storage.tasks = crate::task_execution::decode_tasks(
                    tasks.remove(&storage.task_storage_id).unwrap_or_default(),
                    next,
                )
                .map_err(|_| Error::Dependency)?;
                validated.push(TaskStorageRecordV1 {
                    project_id: project.project_id,
                    proposal_id: proposal.proposal_id,
                    proposal_revision: proposal.revision,
                    storage: storage.clone(),
                });
            }
        }
    }
    if !stored.is_empty() || !tasks.is_empty() {
        return Err(Error::Dependency);
    }
    TaskStateV1 {
        schema_version: 1,
        generation,
        next_task_id: next,
        storages: validated,
    }
    .validate()
    .map_err(|_| Error::Dependency)?;
    Ok(projects)
}
