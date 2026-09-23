use super::{
    models::{ProfilesView, PublicProfilesView, PutProfileRequest},
    store,
};
use crate::{
    auth::Session,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use generated_contracts::PrincipalId;
use std::sync::Arc;

pub(crate) async fn get_me(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<ProfilesView>, Error> {
    Ok(Json(store::read(&app.db, session.view.principal_id).await?))
}

pub(crate) async fn put_me(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Json(request): Json<PutProfileRequest>,
) -> Result<Json<ProfilesView>, Error> {
    let id = session.view.principal_id;
    match request {
        PutProfileRequest::Client(profile) => store::write_client(&app.db, id, profile).await?,
        PutProfileRequest::Worker(profile) => store::write_worker(&app.db, id, profile).await?,
    }
    Ok(Json(store::read(&app.db, id).await?))
}

pub(crate) async fn get_public(
    State(app): State<Arc<App>>,
    Path(principal_id): Path<String>,
) -> Result<Json<PublicProfilesView>, Error> {
    let id = parse::<PrincipalId>(&principal_id)?;
    Ok(Json(store::read(&app.db, id).await?.into()))
}
