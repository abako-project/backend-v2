use super::models::{ProjectBrief, ProjectBriefView, PutProjectBriefRequest};
use crate::state::Error;
use generated_contracts::{EntityId, ProviderInstanceId};
use sqlx::{PgPool, Row, postgres::PgRow};

fn view(row: &PgRow, project_id: EntityId) -> Result<ProjectBriefView, Error> {
    let brief: ProjectBrief = serde_json::from_str(&row.try_get::<String, _>("brief")?)?;
    brief.validate().map_err(|_| Error::Internal)?;
    Ok(ProjectBriefView {
        project_id,
        revision: row.try_get("revision")?,
        brief,
    })
}

pub(super) async fn read(
    pool: &PgPool,
    instance: ProviderInstanceId,
    project: EntityId,
) -> Result<Option<ProjectBriefView>, Error> {
    let row = sqlx::query("SELECT revision, brief::text FROM project_briefs WHERE provider_instance_id=$1 AND project_id=$2")
        .bind(instance.to_string())
        .bind(project.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(|row| view(row, project)).transpose()
}

pub(crate) async fn write(
    pool: &PgPool,
    instance: ProviderInstanceId,
    project: EntityId,
    request: &PutProjectBriefRequest,
) -> Result<(bool, ProjectBriefView), Error> {
    request.validate()?;
    let instance = instance.to_string();
    let id = project.to_string();
    let brief = serde_json::to_string(&request.brief)?;
    let mut tx = pool.begin().await?;
    if request.expected_revision == 0 {
        let created = sqlx::query("INSERT INTO project_briefs (provider_instance_id,project_id,revision,brief) VALUES ($1,$2,1,$3::jsonb) ON CONFLICT DO NOTHING RETURNING revision, brief::text")
            .bind(&instance).bind(&id).bind(&brief)
            .fetch_optional(&mut *tx).await?;
        if let Some(row) = created {
            let result = view(&row, project)?;
            tx.commit().await?;
            return Ok((true, result));
        }
    }
    // Row locks serialize updates and exact retries across adapter replicas.
    let row = sqlx::query("SELECT revision, brief::text FROM project_briefs WHERE provider_instance_id=$1 AND project_id=$2 FOR UPDATE")
        .bind(&instance).bind(&id).fetch_optional(&mut *tx).await?
        .ok_or(Error::Conflict("brief_revision_conflict"))?;
    let current = view(&row, project)?;
    if current.revision == request.expected_revision + 1 && current.brief == request.brief {
        tx.commit().await?;
        return Ok((false, current));
    }
    if current.revision != request.expected_revision {
        return Err(Error::Conflict("brief_revision_conflict"));
    }
    let row = sqlx::query("UPDATE project_briefs SET revision=revision+1,brief=$3::jsonb WHERE provider_instance_id=$1 AND project_id=$2 RETURNING revision,brief::text")
        .bind(&instance).bind(&id).bind(&brief).fetch_one(&mut *tx).await?;
    let result = view(&row, project)?;
    tx.commit().await?;
    Ok((false, result))
}
