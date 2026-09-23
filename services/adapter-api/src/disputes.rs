//! Thin transport mapping; the provider remains the dispute authority.
use crate::state::{App, Error, parse};
use axum::{
    Json,
    extract::{Path, State},
};
use generated_contracts::{DisputeView, EntityId, EvidenceRequest, ProviderCommand};
use std::sync::Arc;

pub(crate) async fn read(app: &App, id: EntityId) -> Result<DisputeView, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/disputes/{id}"),
    )
    .await
}

pub(crate) async fn public_case(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<DisputeView>, Error> {
    Ok(Json(read(&app, parse(&id)?).await?))
}

pub(crate) async fn rejection(
    app: &App,
    id: EntityId,
    request: EvidenceRequest,
) -> Result<ProviderCommand, Error> {
    let snapshot = app.snapshot().await?;
    for project in snapshot.projects {
        for milestone in project.proposals.iter().flat_map(|p| &p.milestones) {
            if milestone.submissions.iter().any(|s| s.submission_id == id) {
                return Ok(ProviderCommand::RejectMilestoneCompletion {
                    project_id: project.project_id,
                    milestone_id: milestone.milestone_id,
                    submission_id: id,
                    request,
                });
            }
        }
    }
    Err(Error::NotFound)
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
