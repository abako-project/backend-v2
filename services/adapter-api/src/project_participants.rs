use crate::{
    auth::Session,
    operations, profiles,
    profiles::models::PublicProfilesView,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use generated_contracts::{AccountId32, EntityId, PrincipalId};
use serde::Serialize;
use sqlx::Row;
use std::sync::Arc;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParticipantView {
    account_id: AccountId32,
    display_name: Option<String>,
    profiles: Option<PublicProfilesView>,
    client_image: bool,
    worker_image: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectParticipantsView {
    client: ParticipantView,
    coordinator: ParticipantView,
}

async fn participant(app: &App, account: AccountId32) -> Result<ParticipantView, Error> {
    let row = sqlx::query("SELECT p.principal_id, p.display_name, c.image_data IS NOT NULL AS client_image, w.image_data IS NOT NULL AS worker_image FROM principals p LEFT JOIN client_profiles c USING (principal_id) LEFT JOIN worker_profiles w USING (principal_id) WHERE p.account_id = $1")
        .bind(account.to_string()).fetch_optional(&app.db).await?;
    let (display_name, public, client_image, worker_image) = if let Some(row) = row {
        let principal = parse::<PrincipalId>(&row.try_get::<String, _>("principal_id")?)?;
        (
            Some(row.try_get("display_name")?),
            Some(profiles::store::read(&app.db, principal).await?.into()),
            row.try_get("client_image")?,
            row.try_get("worker_image")?,
        )
    } else {
        (None, None, false, false)
    };
    Ok(ParticipantView {
        account_id: account,
        display_name,
        profiles: public,
        client_image,
        worker_image,
    })
}

pub(crate) async fn get(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<ProjectParticipantsView>, Error> {
    let id = parse::<EntityId>(&id)?;
    let snapshot = app.snapshot().await?;
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == id && operations::visible(p, session.view.account_id))
        .ok_or(Error::NotFound)?;
    let (client, coordinator) = tokio::try_join!(
        participant(&app, project.client),
        participant(&app, project.coordinator)
    )?;
    Ok(Json(ProjectParticipantsView {
        client,
        coordinator,
    }))
}
