//! Private, append-only discussion bound to a provider delivery version.
use crate::{
    auth::Session,
    state::{App, Error, now, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use generated_contracts::{
    AccountId32, CompletionSubmission, EntityId, ProjectView, ProviderCommand, ProviderInstanceId,
    SubmissionReview,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryComment {
    cursor: i64,
    comment_id: EntityId,
    submission_id: EntityId,
    author_account: AccountId32,
    created_at: i64,
    message: String,
    kind: Kind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Comment,
    Rejection,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostComment {
    comment_id: EntityId,
    message: String,
    kind: Kind,
}
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum RejectRequest {
    Comment {
        #[serde(rename = "commentId")]
        comment_id: EntityId,
    },
    Evidence {
        evidence: generated_contracts::EvidenceReference,
    },
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    #[serde(default)]
    after: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Comments {
    items: Vec<DeliveryComment>,
    next_after: Option<i64>,
}
struct Context {
    instance: ProviderInstanceId,
    project: ProjectView,
    submission: CompletionSubmission,
    milestone_id: EntityId,
    current: bool,
}
async fn context(app: &App, session: &Session, id: EntityId) -> Result<Context, Error> {
    let snapshot = app.snapshot().await?;
    for project in snapshot.projects {
        for milestone in project.proposals.iter().flat_map(|p| &p.milestones) {
            if let Some(submission) = milestone.submissions.iter().find(|s| s.submission_id == id) {
                if session.view.account_id != project.client
                    && session.view.account_id != project.coordinator
                {
                    return Err(Error::NotFound);
                }
                return Ok(Context {
                    instance: snapshot.info.provider_instance_id,
                    submission: submission.clone(),
                    milestone_id: milestone.milestone_id,
                    current: milestone
                        .submissions
                        .last()
                        .is_some_and(|s| s.submission_id == id),
                    project,
                });
            }
        }
    }
    Err(Error::NotFound)
}
fn comment(row: &sqlx::postgres::PgRow) -> Result<DeliveryComment, Error> {
    Ok(DeliveryComment {
        cursor: row.try_get("cursor")?,
        comment_id: parse(&row.try_get::<String, _>("comment_id")?)?,
        submission_id: parse(&row.try_get::<String, _>("submission_id")?)?,
        author_account: parse(&row.try_get::<String, _>("author_account")?)?,
        created_at: row.try_get("created_at")?,
        message: row.try_get("message")?,
        kind: serde_json::from_value(serde_json::Value::String(row.try_get("kind")?))?,
    })
}
pub(crate) async fn get(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Comments>, Error> {
    if cursor.after < 0 {
        return Err(Error::Invalid);
    }
    let id = parse(&id)?;
    let c = context(&app, &session, id).await?;
    let visible_reason = match c.submission.review {
        SubmissionReview::RejectedWithComment { comment_id, .. } => Some(comment_id.to_string()),
        _ => None,
    };
    let rows=sqlx::query("SELECT * FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND cursor>$3 AND (kind='Comment' OR comment_id=$4) ORDER BY cursor LIMIT 21")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(cursor.after).bind(visible_reason).fetch_all(&app.db).await?;
    let items = rows
        .iter()
        .take(20)
        .map(comment)
        .collect::<Result<Vec<_>, _>>()?;
    let next_after = if rows.len() > 20 {
        items.last().map(|i| i.cursor)
    } else {
        None
    };
    Ok(Json(Comments { items, next_after }))
}
pub(crate) async fn post(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Json(request): Json<PostComment>,
) -> Result<(StatusCode, Json<DeliveryComment>), Error> {
    if request.message.trim().is_empty()
        || request.message.len() > 4000
        || request.message.contains('\0')
    {
        return Err(Error::Invalid);
    }
    let id = parse(&id)?;
    let c = context(&app, &session, id).await?;
    let account = session.view.account_id;
    let prior=sqlx::query("SELECT * FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(request.comment_id.to_string()).fetch_optional(&app.db).await?;
    if let Some(row) = prior {
        let saved = comment(&row)?;
        if saved.author_account != account
            || saved.message != request.message
            || saved.kind != request.kind
        {
            return Err(Error::Conflict("submission_comment_conflict"));
        }
        return Ok((StatusCode::OK, Json(saved)));
    }
    if !c.current || c.project.active_dispute_id.is_some() || c.project.cancelled {
        return Err(Error::Conflict("submission_not_current"));
    }
    match request.kind {
        Kind::Rejection
            if account == c.project.client
                && c.submission.review == SubmissionReview::PendingReview => {}
        Kind::Comment
            if matches!(
                c.submission.review,
                SubmissionReview::Rejected { .. }
                    | SubmissionReview::RejectedWithComment { .. }
                    | SubmissionReview::Accepted { .. }
            ) => {}
        _ => return Err(Error::Forbidden),
    }
    let kind = match request.kind {
        Kind::Comment => "Comment",
        Kind::Rejection => "Rejection",
    };
    sqlx::query("INSERT INTO submission_comments (provider_instance_id,submission_id,comment_id,author_account,created_at,message,kind) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(request.comment_id.to_string()).bind(account.to_string()).bind(now()?).bind(&request.message).bind(kind).execute(&app.db).await?;
    let row=sqlx::query("SELECT * FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(request.comment_id.to_string()).fetch_one(&app.db).await?;
    let saved = comment(&row)?;
    if saved.author_account != account
        || saved.message != request.message
        || saved.kind != request.kind
    {
        return Err(Error::Conflict("submission_comment_conflict"));
    }
    Ok((StatusCode::CREATED, Json(saved)))
}
pub(crate) async fn rejection(
    app: &App,
    session: &Session,
    id: EntityId,
    request: RejectRequest,
) -> Result<ProviderCommand, Error> {
    let c = context(app, session, id).await?;
    if session.view.account_id != c.project.client {
        return Err(Error::Forbidden);
    }
    if !c.current {
        return Err(Error::Conflict("submission_not_current"));
    }
    let comment_id = match request {
        RejectRequest::Comment { comment_id } => comment_id,
        RejectRequest::Evidence { evidence } => {
            return Ok(ProviderCommand::RejectMilestoneCompletion {
                project_id: c.project.project_id,
                milestone_id: c.milestone_id,
                submission_id: id,
                request: generated_contracts::EvidenceRequest { evidence },
            });
        }
    };
    let row=sqlx::query("SELECT * FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(comment_id.to_string()).fetch_optional(&app.db).await?.ok_or(Error::NotFound)?;
    let reason = comment(&row)?;
    if reason.author_account != c.project.client || reason.kind != Kind::Rejection {
        return Err(Error::Forbidden);
    }
    Ok(ProviderCommand::RejectMilestoneDelivery {
        project_id: c.project.project_id,
        milestone_id: c.milestone_id,
        submission_id: id,
        comment_id,
    })
}
