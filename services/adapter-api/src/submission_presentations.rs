//! One immutable description per provider-confirmed delivery version.
use crate::{
    auth::Session,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use generated_contracts::{EntityId, ProviderInstanceId};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Details {
    pub(crate) documentation: Option<String>,
    pub(crate) links: Option<String>,
}
impl Details {
    fn validate(&self) -> Result<(), Error> {
        for reference in [&self.documentation, &self.links].into_iter().flatten() {
            if reference.len() > 2048 {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PutPresentation {
    expected_revision: i64,
    #[serde(flatten)]
    details: Details,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Presentation {
    submission_id: EntityId,
    revision: i64,
    #[serde(flatten)]
    details: Details,
}
async fn context(
    app: &App,
    session: &Session,
    id: EntityId,
    writing: bool,
) -> Result<ProviderInstanceId, Error> {
    let snapshot = app.snapshot().await?;
    let account = session.view.account_id;
    for project in &snapshot.projects {
        for submission in project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.submissions)
        {
            if submission.submission_id != id {
                continue;
            }
            if account != project.client && account != project.coordinator {
                return Err(Error::NotFound);
            }
            if writing && account != submission.submitted_by {
                return Err(Error::Forbidden);
            }
            return Ok(snapshot.info.provider_instance_id);
        }
    }
    Err(Error::NotFound)
}
async fn read(
    pool: &PgPool,
    instance: ProviderInstanceId,
    id: EntityId,
) -> Result<Option<Presentation>, Error> {
    let row = sqlx::query("SELECT documentation, links FROM submission_presentations WHERE provider_instance_id=$1 AND submission_id=$2")
        .bind(instance.to_string()).bind(id.to_string()).fetch_optional(pool).await?;
    row.map(|r| {
        Ok(Presentation {
            submission_id: id,
            revision: 1,
            details: Details {
                documentation: r.try_get("documentation")?,
                links: r.try_get("links")?,
            },
        })
    })
    .transpose()
}
pub(crate) async fn get(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Option<Presentation>>, Error> {
    let id = parse(&id)?;
    let instance = context(&app, &session, id, false).await?;
    Ok(Json(read(&app.db, instance, id).await?))
}
pub(crate) async fn put(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Json(request): Json<PutPresentation>,
) -> Result<(StatusCode, Json<Presentation>), Error> {
    let id = parse(&id)?;
    let instance = context(&app, &session, id, true).await?;
    request.details.validate()?;
    if request.expected_revision != 0 {
        return Err(Error::Conflict("submission_presentation_immutable"));
    }
    let result = sqlx::query("INSERT INTO submission_presentations (provider_instance_id,submission_id,documentation,links) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(instance.to_string()).bind(id.to_string()).bind(&request.details.documentation).bind(&request.details.links).execute(&app.db).await?;
    let saved = read(&app.db, instance, id).await?.ok_or(Error::Internal)?;
    if saved.details != request.details {
        return Err(Error::Conflict("submission_presentation_immutable"));
    }
    Ok((
        if result.rows_affected() == 1 {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(saved),
    ))
}
