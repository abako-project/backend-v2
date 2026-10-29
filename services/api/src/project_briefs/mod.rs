pub(crate) mod models;
pub(crate) mod store;

use crate::{
    auth::Session,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use models::{ProjectBriefView, PutProjectBriefRequest};
use protocol::{EntityId, ProviderInstanceId};
use std::sync::Arc;

async fn context(
    app: &App,
    session: &Session,
    id: &str,
    writing: bool,
) -> Result<(ProviderInstanceId, EntityId), Error> {
    let id = parse::<EntityId>(id)?;
    let snapshot = app.snapshot().await?;
    let project = snapshot
        .projects
        .into_iter()
        .find(|project| project.project_id == id)
        .ok_or(Error::NotFound)?;
    let project = crate::tasks::views::project(project, session.view.account_id);
    let project = project.ok_or(Error::NotFound)?;
    if writing && project.client != session.view.account_id {
        return Err(Error::Forbidden);
    }
    Ok((snapshot.info.provider_instance_id, id))
}

pub(crate) async fn get(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Option<ProjectBriefView>>, Error> {
    let (instance, project) = context(&app, &session, &id, false).await?;
    Ok(Json(store::read(&app.db, instance, project).await?))
}

pub(crate) async fn put(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Json(request): Json<PutProjectBriefRequest>,
) -> Result<(StatusCode, Json<ProjectBriefView>), Error> {
    let (instance, project) = context(&app, &session, &id, true).await?;
    if !crate::platform::allows_project_type(
        &app.config.platform,
        request.brief.project_type.as_str(),
    ) {
        return Err(Error::Invalid);
    }
    let (created, brief) = store::write(&app.db, instance, project, &request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(brief),
    ))
}

#[cfg(test)]
mod tests;
