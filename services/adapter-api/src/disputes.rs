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

pub(crate) async fn public_read(app: &App, id: EntityId) -> Result<DisputeView, Error> {
    let mut view = read(app, id).await?;
    let generation = app.snapshot().await?.info.provider_instance_id;
    crate::evidence::case(app, generation, &mut view).await?;
    Ok(view)
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
