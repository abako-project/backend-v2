//! Public project composition joins verified metadata with compact core facts.
use crate::state::{App, Error};
use protocol::{ProjectView, ProviderInstanceId, provider_core::ProjectCore};
use std::collections::BTreeMap;

/// Caller has already applied the public visibility rule to this core entity.
pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    project: ProjectCore,
) -> Result<ProjectView, Error> {
    let tasks = crate::tasks::views::content(app, generation, project.clone()).await?;
    let mut tasks: BTreeMap<_, _> = tasks
        .storages
        .into_iter()
        .map(|record| (record.storage.task_storage_id, record.storage))
        .collect();
    let mut project = project;
    let (title, description) = crate::creation_metadata::project_text(
        app,
        generation,
        project.project_id,
        project.client,
        &project.content,
    )
    .await?;
    let mut proposals = Vec::with_capacity(project.proposals.len());
    for proposal in std::mem::take(&mut project.proposals) {
        let content = proposal
            .milestones
            .iter()
            .map(|m| {
                tasks
                    .remove(&m.task_storage.task_storage_id)
                    .ok_or(Error::Dependency)
            })
            .collect::<Result<Vec<_>, _>>()?;
        proposals.push(
            crate::proposal_metadata::compose(
                app,
                generation,
                project.project_id,
                project.coordinator,
                project.client,
                proposal,
                content,
            )
            .await?,
        );
    }
    Ok(project.into_view(title, description, proposals))
}
