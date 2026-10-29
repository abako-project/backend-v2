//! Thin transport mapping; the provider remains the dispute authority.
use crate::state::{App, Error, parse};
use axum::{
    Json,
    extract::{Path, State},
};
use protocol::{DisputeView, EntityId, EvidenceRequest, ProviderCommand};
use std::sync::Arc;

pub(crate) async fn read(
    app: &App,
    id: EntityId,
) -> Result<protocol::provider_core::CoreDisputeView, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/disputes/{id}"),
    )
    .await
}

pub(crate) async fn public_read(app: &App, id: EntityId) -> Result<DisputeView, Error> {
    let (generation, mut view) = present(app, read(app, id).await?).await?;
    let _verified_text = crate::evidence::case(app, generation, &mut view).await?;
    Ok(view)
}

/// Public case presentation resolves only the selected frozen milestone title.
/// Signing aliases and private conversations continue to use `read` directly.
pub(crate) async fn present(
    app: &App,
    view: protocol::provider_core::CoreDisputeView,
) -> Result<(protocol::ProviderInstanceId, DisputeView), Error> {
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == view.dispute.project_id)
        .ok_or(Error::Dependency)?;
    let proposal = project
        .proposals
        .iter()
        .find(|p| {
            p.milestones
                .iter()
                .any(|m| m.milestone_id == view.milestone.milestone_id)
        })
        .ok_or(Error::Dependency)?;
    let milestone = proposal
        .milestones
        .iter()
        .find(|m| m.milestone_id == view.milestone.milestone_id)
        .ok_or(Error::Dependency)?;
    if *milestone != view.milestone || proposal.revision != view.dispute.proposal_revision {
        return Err(Error::Dependency);
    }
    let definition = crate::proposal_metadata::definition(
        app,
        generation,
        project.project_id,
        project.coordinator,
        proposal.clone(),
    )
    .await?;
    let title = definition
        .milestones
        .into_iter()
        .find(|m| m.key == view.milestone.definition.key)
        .ok_or(Error::Dependency)?
        .title;
    let tasks =
        crate::tasks::freezes::compose_case(app, generation, proposal.proposal_id, view.clone())
            .await?;
    Ok((
        generation,
        DisputeView {
            dispute: view.dispute,
            milestone: view
                .milestone
                .into_view(title, tasks)
                .map_err(|_| Error::Dependency)?,
        },
    ))
}

pub(crate) async fn public_case(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<DisputeView>, Error> {
    Ok(Json(public_read(&app, parse(&id)?).await?))
}

pub(crate) async fn response(
    app: &App,
    id: EntityId,
    request: EvidenceRequest,
) -> Result<ProviderCommand, Error> {
    let case = read(app, id).await?;
    Ok(ProviderCommand::RespondDispute {
        project_id: case.dispute.project_id,
        dispute_id: id,
        request,
    })
}

pub(crate) mod conversations;
