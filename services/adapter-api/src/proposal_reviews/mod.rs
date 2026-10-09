pub(crate) mod models;
mod store;

use crate::{
    auth::Session,
    operations,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use generated_contracts::{
    AccountId32, EntityId, ProposalStatus, ProposalView, ProviderInstanceId,
};
use models::{CommentsPage, Cursor, PostComment, PresentationView, PutPresentation, ReviewComment};
use std::sync::Arc;

pub(super) struct Context {
    instance: ProviderInstanceId,
    project_id: EntityId,
    proposal: ProposalView,
    account: AccountId32,
}
#[derive(Clone, Copy)]
enum Access {
    Participant,
    Coordinator,
    Review,
}
async fn context(
    app: &App,
    session: &Session,
    ids: &(String, String),
    access: Access,
    writing: bool,
) -> Result<Context, Error> {
    let project_id = parse::<EntityId>(&ids.0)?;
    let proposal_id = parse::<EntityId>(&ids.1)?;
    let snapshot = app.snapshot().await?;
    let account = session.view.account_id;
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == project_id && operations::visible(p, account))
        .ok_or(Error::NotFound)?;
    let proposal = project
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .ok_or(Error::NotFound)?;
    if matches!(access, Access::Coordinator) && account != project.coordinator {
        return Err(Error::Forbidden);
    }
    if matches!(access, Access::Review)
        && account != project.client
        && account != project.coordinator
    {
        return Err(Error::Forbidden);
    }
    if writing
        && (project.cancelled
            || project.planning.frozen
            || project.active_dispute_id.is_some()
            || proposal.status == ProposalStatus::Cancelled)
    {
        return Err(Error::Conflict("proposal_read_only"));
    }
    if writing && matches!(access, Access::Coordinator) && proposal.status != ProposalStatus::Draft
    {
        return Err(Error::Conflict("proposal_not_draft"));
    }
    Ok(Context {
        instance: snapshot.info.provider_instance_id,
        project_id,
        proposal: proposal.clone(),
        account,
    })
}
pub(crate) async fn get_presentation(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(ids): Path<(String, String)>,
) -> Result<Json<Option<PresentationView>>, Error> {
    let context = context(&app, &session, &ids, Access::Participant, false).await?;
    Ok(Json(store::presentation(&app.db, &context).await?))
}
pub(crate) async fn put_presentation(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(ids): Path<(String, String)>,
    Json(request): Json<PutPresentation>,
) -> Result<(StatusCode, Json<PresentationView>), Error> {
    let context = context(&app, &session, &ids, Access::Coordinator, true).await?;
    let (created, result) = store::put_presentation(&app.db, &context, &request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(result),
    ))
}
pub(crate) async fn get_comments(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(ids): Path<(String, String)>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<CommentsPage>, Error> {
    if cursor.after < 0 {
        return Err(Error::Invalid);
    }
    let context = context(&app, &session, &ids, Access::Review, false).await?;
    Ok(Json(
        store::comments(&app.db, &context, cursor.after).await?,
    ))
}
pub(crate) async fn post_comment(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(ids): Path<(String, String)>,
    Json(request): Json<PostComment>,
) -> Result<(StatusCode, Json<ReviewComment>), Error> {
    let context = context(&app, &session, &ids, Access::Review, true).await?;
    let (created, result) = store::post_comment(&app.db, &context, &request).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(result),
    ))
}
#[cfg(test)]
mod tests;
