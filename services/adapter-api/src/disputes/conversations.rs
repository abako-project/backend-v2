//! Public immutable arguments and a private conversation for provider-owned cases.
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
    AccountId32, DisputeView, EntityId, OpenDisputeWithCommentRequest, ProposalStatus,
    ProviderCommand, ProviderInstanceId, SubmissionReview,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::BTreeSet, sync::Arc};

use super::read;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenRequest {
    project_id: EntityId,
    milestone_id: EntityId,
    rejected_submission_id: EntityId,
    reason: String,
}
fn text(value: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > 4000 || value.contains('\0') {
        return Err(Error::Invalid);
    }
    Ok(())
}
pub(crate) async fn opening(
    app: &App,
    session: &Session,
    request: OpenRequest,
    key: &str,
) -> Result<ProviderCommand, Error> {
    text(&request.reason)?;
    let comment_id: EntityId = parse(key)?;
    let snapshot = app.snapshot().await?;
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == request.project_id)
        .ok_or(Error::NotFound)?;
    let account = session.view.account_id;
    if account != project.client && account != project.coordinator {
        return Err(Error::Forbidden);
    }
    let instance = snapshot.info.provider_instance_id.to_string();
    // The provider rechecks current rejection and freezes atomically; persist first
    // so no confirmed case can be left without its written opening argument.
    sqlx::query("INSERT INTO dispute_opening_arguments (provider_instance_id,comment_id,project_id,milestone_id,submission_id,author_account,reason) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(&instance).bind(comment_id.to_string()).bind(request.project_id.to_string()).bind(request.milestone_id.to_string()).bind(request.rejected_submission_id.to_string()).bind(account.to_string()).bind(&request.reason).execute(&app.db).await?;
    let row = sqlx::query(
        "SELECT * FROM dispute_opening_arguments WHERE provider_instance_id=$1 AND comment_id=$2",
    )
    .bind(&instance)
    .bind(comment_id.to_string())
    .fetch_one(&app.db)
    .await?;
    for (column, expected) in [
        ("project_id", request.project_id.to_string()),
        ("milestone_id", request.milestone_id.to_string()),
        ("submission_id", request.rejected_submission_id.to_string()),
        ("author_account", account.to_string()),
        ("reason", request.reason),
    ] {
        if row.try_get::<String, _>(column)? != expected {
            return Err(Error::Conflict("dispute_opening_conflict"));
        }
    }
    Ok(ProviderCommand::OpenDisputeWithComment(
        OpenDisputeWithCommentRequest {
            project_id: request.project_id,
            milestone_id: request.milestone_id,
            rejected_submission_id: request.rejected_submission_id,
            comment_id,
        },
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Author {
    account_id: AccountId32,
    display_name: Option<String>,
    image_url: Option<String>,
    role: &'static str,
}
async fn author(app: &App, account: AccountId32, role: &'static str) -> Result<Author, Error> {
    let row=sqlx::query("SELECT p.principal_id,p.display_name,c.image_data IS NOT NULL AS client_image,w.image_data IS NOT NULL AS worker_image FROM principals p LEFT JOIN client_profiles c USING(principal_id) LEFT JOIN worker_profiles w USING(principal_id) WHERE p.account_id=$1")
        .bind(account.to_string()).fetch_optional(&app.db).await?;
    let mut result = Author {
        account_id: account,
        display_name: None,
        image_url: None,
        role,
    };
    if let Some(row) = row {
        result.display_name = Some(row.try_get("display_name")?);
        let section = if role == "client" { "client" } else { "worker" };
        if row.try_get::<bool, _>(format!("{section}_image").as_str())? {
            let principal: String = row.try_get("principal_id")?;
            result.image_url = Some(format!("/api/profiles/{principal}/{section}/image"));
        }
    }
    Ok(result)
}
struct Context {
    instance: ProviderInstanceId,
    case: DisputeView,
    title: String,
    client: Author,
    coordinator: Author,
    workers: Vec<Author>,
}
impl Context {
    fn participant(&self, account: AccountId32) -> Result<Author, Error> {
        [&self.client, &self.coordinator]
            .into_iter()
            .chain(&self.workers)
            .find(|author| author.account_id == account)
            .cloned()
            .ok_or(Error::NotFound)
    }
    fn can_write_channel(&self, app: &App, account: AccountId32) -> bool {
        account == self.coordinator.account_id
            || (app.config.dispute_channel_allow_participants && self.participant(account).is_ok())
    }
    fn read_channel(&self, app: &App, account: AccountId32) -> Result<(), Error> {
        if app.config.dispute_channel_allow_participants {
            self.participant(account).map(|_| ())
        } else {
            self.party(account)
        }
    }
    fn party(&self, account: AccountId32) -> Result<(), Error> {
        if account != self.client.account_id && account != self.coordinator.account_id {
            return Err(Error::NotFound);
        }
        Ok(())
    }
}
async fn context(app: &App, id: EntityId) -> Result<Context, Error> {
    let case = read(app, id).await?;
    let snapshot = app.snapshot().await?;
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == case.dispute.project_id)
        .ok_or(Error::NotFound)?;
    let (client, coordinator) = tokio::try_join!(
        author(app, project.client, "client"),
        author(app, project.coordinator, "consultant")
    )?;
    let accounts: BTreeSet<_> = project
        .proposals
        .iter()
        .filter(|p| p.status == ProposalStatus::Approved)
        .flat_map(|p| &p.milestones)
        .flat_map(|m| &m.assignments)
        .map(|a| a.worker)
        .filter(|a| *a != project.client && *a != project.coordinator)
        .collect();
    let mut workers = Vec::with_capacity(accounts.len());
    for account in accounts {
        workers.push(author(app, account, "worker").await?);
    }
    Ok(Context {
        instance: snapshot.info.provider_instance_id,
        title: project.title.clone(),
        case,
        client,
        coordinator,
        workers,
    })
}
async fn opening_reason(app: &App, c: &Context) -> Result<String, Error> {
    if let Some(id) = c.case.dispute.opening_comment_id {
        let row=sqlx::query("SELECT reason,project_id,milestone_id,submission_id,author_account FROM dispute_opening_arguments WHERE provider_instance_id=$1 AND comment_id=$2")
            .bind(c.instance.to_string()).bind(id.to_string()).fetch_optional(&app.db).await?.ok_or(Error::Dependency)?;
        let d = &c.case.dispute;
        for (column, expected) in [
            ("project_id", d.project_id.to_string()),
            ("milestone_id", d.milestone_id.to_string()),
            ("submission_id", d.rejected_submission_id.to_string()),
            ("author_account", d.opened_by.to_string()),
        ] {
            if row.try_get::<String, _>(column)? != expected {
                return Err(Error::Dependency);
            }
        }
        return Ok(row.try_get("reason")?);
    }
    c.case
        .dispute
        .evidence
        .as_ref()
        .map(|e| e.url().to_owned())
        .ok_or(Error::Dependency)
}
pub(crate) async fn presentation(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    let c = context(&app, parse(&id)?).await?;
    let reason = opening_reason(&app, &c).await?;
    let mut value = serde_json::to_value(&c.case)?;
    value["presentation"] = json!({"projectTitle":c.title,"openingReason":reason,"client":c.client,"coordinator":c.coordinator});
    Ok(Json(value))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    #[serde(rename = "RESPONSE")]
    Response,
    #[serde(rename = "ADDITIONAL")]
    Additional,
    #[serde(rename = "MESSAGE")]
    Message,
}
impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::Response => "RESPONSE",
            Self::Additional => "ADDITIONAL",
            Self::Message => "MESSAGE",
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostEntry {
    entry_id: EntityId,
    content: String,
    argument_type: Kind,
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    #[serde(default)]
    after: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_field_names,
    reason = "entryId is the public immutable entry identity."
)]
pub(crate) struct Entry {
    cursor: i64,
    entry_id: EntityId,
    dispute_id: EntityId,
    author: Author,
    created_at: i64,
    content: String,
    argument_type: Kind,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    items: Vec<Entry>,
    next_after: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    can_write: Option<bool>,
}
fn entry(row: &sqlx::postgres::PgRow, c: &Context) -> Result<Entry, Error> {
    let account: AccountId32 = parse(&row.try_get::<String, _>("author_account")?)?;
    let author = c.participant(account)?;
    Ok(Entry {
        cursor: row.try_get("cursor")?,
        entry_id: parse(&row.try_get::<String, _>("entry_id")?)?,
        dispute_id: c.case.dispute.dispute_id,
        author,
        created_at: row.try_get("created_at")?,
        content: row.try_get("content")?,
        argument_type: serde_json::from_value(Value::String(row.try_get("kind")?))?,
    })
}
async fn entries(app: &App, c: &Context, cursor: Cursor, private: bool) -> Result<Page, Error> {
    if cursor.after < 0 {
        return Err(Error::Invalid);
    }
    let rows=sqlx::query("SELECT * FROM dispute_entries WHERE provider_instance_id=$1 AND dispute_id=$2 AND cursor>$3 AND (kind='MESSAGE')=$4 ORDER BY cursor LIMIT 21")
        .bind(c.instance.to_string()).bind(c.case.dispute.dispute_id.to_string()).bind(cursor.after).bind(private).fetch_all(&app.db).await?;
    let items = rows
        .iter()
        .take(20)
        .map(|r| entry(r, c))
        .collect::<Result<Vec<_>, _>>()?;
    let next_after = if rows.len() > 20 {
        items.last().map(|i| i.cursor)
    } else {
        None
    };
    Ok(Page {
        items,
        next_after,
        can_write: None,
    })
}
pub(crate) async fn arguments(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Page>, Error> {
    let c = context(&app, parse(&id)?).await?;
    Ok(Json(entries(&app, &c, cursor, false).await?))
}
pub(crate) async fn messages(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Page>, Error> {
    let c = context(&app, parse(&id)?).await?;
    c.read_channel(&app, session.view.account_id)?;
    let mut page = entries(&app, &c, cursor, true).await?;
    page.can_write = Some(c.can_write_channel(&app, session.view.account_id));
    Ok(Json(page))
}
async fn post(
    app: &App,
    session: &Session,
    id: EntityId,
    request: PostEntry,
    private: bool,
) -> Result<(StatusCode, Json<Entry>), Error> {
    text(&request.content)?;
    if (request.argument_type == Kind::Message) != private {
        return Err(Error::Invalid);
    }
    let c = context(app, id).await?;
    if private {
        c.read_channel(app, session.view.account_id)?;
        if !c.can_write_channel(app, session.view.account_id) {
            return Err(Error::Forbidden);
        }
    } else {
        c.party(session.view.account_id)?;
    }
    let result=sqlx::query("INSERT INTO dispute_entries (provider_instance_id,dispute_id,entry_id,author_account,created_at,kind,content) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(request.entry_id.to_string()).bind(session.view.account_id.to_string()).bind(now()?).bind(request.argument_type.name()).bind(&request.content).execute(&app.db).await?;
    let row=sqlx::query("SELECT * FROM dispute_entries WHERE provider_instance_id=$1 AND dispute_id=$2 AND entry_id=$3")
        .bind(c.instance.to_string()).bind(id.to_string()).bind(request.entry_id.to_string()).fetch_one(&app.db).await?;
    let saved = entry(&row, &c)?;
    if saved.author.account_id != session.view.account_id
        || saved.content != request.content
        || saved.argument_type != request.argument_type
    {
        return Err(Error::Conflict("dispute_entry_immutable"));
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
pub(crate) async fn post_argument(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Json(request): Json<PostEntry>,
) -> Result<(StatusCode, Json<Entry>), Error> {
    post(&app, &session, parse(&id)?, request, false).await
}
pub(crate) async fn post_message(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Json(request): Json<PostEntry>,
) -> Result<(StatusCode, Json<Entry>), Error> {
    post(&app, &session, parse(&id)?, request, true).await
}

pub(crate) async fn history(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    let c = context(&app, parse(&id)?).await?;
    let mut deliverables = Vec::new();
    let mut rejections = Vec::new();
    let mut events = Vec::new();
    let dispute_id = c.case.dispute.dispute_id;
    for s in &c.case.milestone.submissions {
        let row=sqlx::query("SELECT documentation,links FROM submission_presentations WHERE provider_instance_id=$1 AND submission_id=$2").bind(c.instance.to_string()).bind(s.submission_id.to_string()).fetch_optional(&app.db).await?;
        let documentation = if let Some(r) = &row {
            r.try_get::<Option<String>, _>("documentation")?
        } else {
            s.deliverable.as_ref().map(|e| e.url().to_owned())
        };
        let links = if let Some(r) = &row {
            r.try_get::<Option<String>, _>("links")?
        } else {
            None
        };
        deliverables.push(json!({"version":s.version.to_string(),"submittedAt":s.submitted_at,"documentation":documentation,"links":links,"developerName":c.coordinator.display_name}));
        events.push(json!({"id":s.submission_id,"disputeId":dispute_id,"eventType":"MilestoneSubmitted","eventData":{"version":s.version},"createdAt":s.submitted_at}));
        let rejected = match &s.review {
            SubmissionReview::Rejected {
                reason,
                reviewed_at,
                ..
            } => Some((reason.url().to_owned(), *reviewed_at)),
            SubmissionReview::RejectedWithComment {
                comment_id,
                reviewed_at,
                ..
            } => {
                let row=sqlx::query("SELECT message FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3 AND author_account=$4 AND kind='Rejection'").bind(c.instance.to_string()).bind(s.submission_id.to_string()).bind(comment_id.to_string()).bind(c.client.account_id.to_string()).fetch_optional(&app.db).await?.ok_or(Error::Dependency)?;
                Some((row.try_get::<String, _>("message")?, *reviewed_at))
            }
            _ => None,
        };
        if let Some((reason, at)) = rejected {
            rejections.push(json!({"rejectedAt":at,"rejectedBy":c.client.account_id,"rejectedByName":c.client.display_name,"reason":reason,"version":s.version.to_string()}));
            events.push(json!({"id":format!("{}-review",s.submission_id),"disputeId":dispute_id,"eventType":"MilestoneRejected","eventData":{"version":s.version},"createdAt":at}));
        }
    }
    events.push(json!({"id":dispute_id,"disputeId":dispute_id,"eventType":"DisputeOpened","eventData":{},"createdAt":c.case.dispute.opened_at}));
    let rows = sqlx::query("SELECT * FROM dispute_entries WHERE provider_instance_id=$1 AND dispute_id=$2 AND kind IN ('RESPONSE','ADDITIONAL') ORDER BY cursor")
        .bind(c.instance.to_string()).bind(dispute_id.to_string()).fetch_all(&app.db).await?;
    for row in &rows {
        let e = entry(row, &c)?;
        events.push(json!({"id":e.entry_id,"disputeId":dispute_id,"eventType":"DisputeArgumentAdded","eventData":{"argumentId":e.entry_id,"author":e.author.display_name,"argumentType":e.argument_type},"createdAt":e.created_at}));
    }
    events.sort_by_key(|event| event["createdAt"].as_i64());
    Ok(Json(
        json!({"project":{"id":c.case.dispute.project_id,"title":c.title,"clientId":c.client.account_id,"clientName":c.client.display_name,"consultantId":c.coordinator.account_id,"consultantName":c.coordinator.display_name},"milestone":{"id":c.case.milestone.milestone_id,"title":c.case.milestone.definition.title,"state":"Disputed"},"deliverables":deliverables,"rejections":rejections,"events":events}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_accepts_prose_and_protocols_without_weakening_limits() {
        assert!(text("Written reason\nipfs://documents sftp://host/file").is_ok());
        assert!(text("  ").is_err());
        assert!(text("bad\0text").is_err());
        assert!(text(&"é".repeat(2000)).is_ok());
        assert!(text(&"é".repeat(2001)).is_err());
        assert!(serde_json::from_value::<PostEntry>(json!({"entryId":"0x00000000000000000000000000000001","content":"x","argumentType":"OPENING"})).is_err());
    }
}
