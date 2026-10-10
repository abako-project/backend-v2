use super::{
    Context,
    models::{
        CommentsPage, PostComment, PresentationView, PutPresentation, ReviewComment, definition,
    },
};
use crate::state::{Error, now, parse};
use generated_contracts::ProposalDefinition;
use sqlx::{PgPool, Row, postgres::PgRow};

fn view(row: &PgRow, context: &Context) -> Result<PresentationView, Error> {
    let saved: ProposalDefinition = serde_json::from_str(&row.try_get::<String, _>("definition")?)?;
    Ok(PresentationView {
        proposal_id: context.proposal.proposal_id,
        revision: row.try_get("revision")?,
        proposal_revision: u64::try_from(row.try_get::<i64, _>("proposal_revision")?)
            .map_err(|_| Error::Internal)?,
        matches_current_definition: saved == definition(&context.proposal),
        milestones: serde_json::from_str(&row.try_get::<String, _>("milestones")?)?,
    })
}
fn comment(row: &PgRow) -> Result<ReviewComment, Error> {
    Ok(ReviewComment {
        comment_id: row.try_get("comment_id")?,
        proposal_revision: u64::try_from(row.try_get::<i64, _>("proposal_revision")?)
            .map_err(|_| Error::Internal)?,
        author_account: parse(&row.try_get::<String, _>("author_account")?)?,
        created_at: row.try_get("created_at")?,
        message: row.try_get("message")?,
        definition: serde_json::from_str(&row.try_get::<String, _>("definition")?)?,
        delivery: row
            .try_get::<Option<String>, _>("delivery")?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        presentation: row
            .try_get::<Option<String>, _>("presentation")?
            .map(|v| serde_json::from_str(&v))
            .transpose()?,
    })
}
pub(super) async fn presentation(
    pool: &PgPool,
    c: &Context,
) -> Result<Option<PresentationView>, Error> {
    let row = sqlx::query("SELECT revision, proposal_revision, definition::text, milestones::text FROM proposal_presentations WHERE provider_instance_id=$1 AND project_id=$2 AND proposal_id=$3 ORDER BY revision DESC LIMIT 1")
        .bind(c.instance.to_string()).bind(c.project_id.to_string()).bind(c.proposal.proposal_id.to_string()).fetch_optional(pool).await?;
    row.as_ref().map(|row| view(row, c)).transpose()
}
pub(super) async fn put_presentation(
    pool: &PgPool,
    c: &Context,
    request: &PutPresentation,
) -> Result<(bool, PresentationView), Error> {
    request.validate(&c.proposal)?;
    let instance = c.instance.to_string();
    let project = c.project_id.to_string();
    let proposal = c.proposal.proposal_id.to_string();
    let mut tx = pool.begin().await?;
    // Append-only versions need a scope lock, including the first insert.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("presentation:{instance}:{project}:{proposal}"))
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT revision, proposal_revision, definition::text, milestones::text FROM proposal_presentations WHERE provider_instance_id=$1 AND project_id=$2 AND proposal_id=$3 ORDER BY revision DESC LIMIT 1")
        .bind(&instance).bind(&project).bind(&proposal).fetch_optional(&mut *tx).await?;
    let current = row.as_ref().map(|r| view(r, c)).transpose()?;
    if let Some(current) = &current
        && Some(current.revision) == request.expected_presentation_revision.checked_add(1)
        && current.proposal_revision == request.expected_proposal_revision
        && current.milestones == request.milestones
    {
        tx.commit().await?;
        return Ok((false, current.clone()));
    }
    let revision = current.as_ref().map_or(0, |p| p.revision);
    if revision != request.expected_presentation_revision
        || c.proposal.revision != request.expected_proposal_revision
    {
        return Err(Error::Conflict("presentation_revision_conflict"));
    }
    let next = revision.checked_add(1).ok_or(Error::Internal)?;
    sqlx::query("INSERT INTO proposal_presentations (provider_instance_id,project_id,proposal_id,revision,proposal_revision,definition,milestones) VALUES ($1,$2,$3,$4,$5,$6::jsonb,$7::jsonb)")
        .bind(&instance).bind(&project).bind(&proposal).bind(next).bind(i64::try_from(c.proposal.revision).map_err(|_|Error::Invalid)?)
        .bind(serde_json::to_string(&definition(&c.proposal))?).bind(serde_json::to_string(&request.milestones)?).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((
        current.is_none(),
        PresentationView {
            proposal_id: c.proposal.proposal_id,
            revision: next,
            proposal_revision: c.proposal.revision,
            matches_current_definition: true,
            milestones: request.milestones.clone(),
        },
    ))
}
pub(super) async fn comments(
    pool: &PgPool,
    c: &Context,
    after: i64,
) -> Result<CommentsPage, Error> {
    let rows = sqlx::query("SELECT comment_id,proposal_revision,author_account,created_at,message,definition::text,presentation::text,delivery::text FROM proposal_review_comments WHERE provider_instance_id=$1 AND project_id=$2 AND proposal_id=$3 AND comment_id>$4 ORDER BY comment_id LIMIT 21")
        .bind(c.instance.to_string()).bind(c.project_id.to_string()).bind(c.proposal.proposal_id.to_string()).bind(after).fetch_all(pool).await?;
    let has_more = rows.len() > 20;
    let items = rows
        .iter()
        .take(20)
        .map(comment)
        .collect::<Result<Vec<_>, _>>()?;
    let next_after = if has_more {
        items.last().map(|c| c.comment_id)
    } else {
        None
    };
    Ok(CommentsPage { items, next_after })
}
pub(super) async fn post_comment(
    pool: &PgPool,
    c: &Context,
    request: &PostComment,
) -> Result<(bool, ReviewComment), Error> {
    request.validate()?;
    let presentation = presentation(pool, c)
        .await?
        .filter(|p| p.matches_current_definition);
    let instance = c.instance.to_string();
    let project = c.project_id.to_string();
    let proposal = c.proposal.proposal_id.to_string();
    let author = c.account.to_string();
    let request_id = request.request_id.to_string();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "comment:{instance}:{project}:{proposal}:{author}:{request_id}"
        ))
        .execute(&mut *tx)
        .await?;
    let prior=sqlx::query("SELECT comment_id,proposal_revision,author_account,created_at,message,definition::text,presentation::text,delivery::text FROM proposal_review_comments WHERE provider_instance_id=$1 AND project_id=$2 AND proposal_id=$3 AND author_account=$4 AND request_id=$5")
        .bind(&instance).bind(&project).bind(&proposal).bind(&author).bind(&request_id).fetch_optional(&mut *tx).await?;
    if let Some(row) = prior {
        let saved = comment(&row)?;
        if saved.message != request.message
            || saved.proposal_revision != request.expected_proposal_revision
        {
            return Err(Error::Conflict("review_request_conflict"));
        }
        tx.commit().await?;
        return Ok((false, saved));
    }
    if c.proposal.revision != request.expected_proposal_revision {
        return Err(Error::Conflict("proposal_revision_conflict"));
    }
    let row=sqlx::query("INSERT INTO proposal_review_comments (provider_instance_id,project_id,proposal_id,request_id,author_account,proposal_revision,created_at,message,definition,presentation,delivery) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::jsonb,$10::jsonb,$11::jsonb) RETURNING comment_id,proposal_revision,author_account,created_at,message,definition::text,presentation::text,delivery::text")
        .bind(&instance).bind(&project).bind(&proposal).bind(&request_id).bind(&author).bind(i64::try_from(c.proposal.revision).map_err(|_|Error::Invalid)?).bind(now()?).bind(&request.message)
        .bind(serde_json::to_string(&definition(&c.proposal))?).bind(presentation.map(|p|serde_json::to_string(&p)).transpose()?).bind(c.proposal.delivery.as_ref().map(serde_json::to_string).transpose()?).fetch_one(&mut *tx).await?;
    let saved = comment(&row)?;
    tx.commit().await?;
    Ok((true, saved))
}
