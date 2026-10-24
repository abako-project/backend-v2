//! R1 project-content copies. Provider facts still select and verify the version.
use crate::state::{App, Error, hash};
use generated_contracts::{
    EntityId, ProjectView, ProviderInstanceId, TaskStorageView, provider_core::ProjectCore,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    proposals: Vec<Proposal>,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    id: EntityId,
    revision: u64,
    milestones: Vec<Milestone>,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Milestone {
    id: EntityId,
    task_storage: TaskStorageView,
}
impl Content {
    fn source(project: &ProjectCore) -> Self {
        Self {
            proposals: project
                .proposals
                .iter()
                .map(|p| Proposal {
                    id: p.proposal_id,
                    revision: p.revision,
                    milestones: p
                        .milestones
                        .iter()
                        .map(|m| Milestone {
                            id: m.milestone_id,
                            task_storage: m.task_storage.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    /// Called only after exact equality against the provider-selected source.
    fn apply(self, project: &mut ProjectCore) {
        for (source, target) in self.proposals.into_iter().zip(&mut project.proposals) {
            for (source, target) in source.milestones.into_iter().zip(&mut target.milestones) {
                target.task_storage = source.task_storage;
            }
        }
    }
}

/// Backfill one authorized view and compose its descriptive fields from the copy.
/// Immutable fingerprints allow concurrent older/newer reads without a last-writer
/// pointer. This is a temporary R1 copy, not an onchain content commitment.
pub(crate) async fn children(
    app: &App,
    generation: ProviderInstanceId,
    project: ProjectCore,
) -> Result<ProjectCore, Error> {
    // Large retained text must not occupy a Tokio core during copying/encoding.
    let (project, expected, payload, fingerprint) = tokio::task::spawn_blocking(move || {
        let expected = Content::source(&project);
        let payload = serde_json::to_string(&expected)?;
        let fingerprint = hash(
            &[
                b"abako/project-task-copy/v3\0".as_slice(),
                generation.as_bytes(),
                project.project_id.as_bytes(),
                payload.as_bytes(),
            ]
            .concat(),
        );
        Ok::<_, Error>((project, expected, payload, fingerprint))
    })
    .await??;
    sqlx::query("INSERT INTO project_content_copies(provider_instance_id,project_id,source_fingerprint,payload) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(generation.to_string()).bind(project.project_id.to_string()).bind(fingerprint.as_slice()).bind(&payload).execute(&app.db).await?;
    let row = sqlx::query("SELECT payload FROM project_content_copies WHERE provider_instance_id=$1 AND project_id=$2 AND source_fingerprint=$3")
        .bind(generation.to_string()).bind(project.project_id.to_string()).bind(fingerprint.as_slice()).fetch_one(&app.db).await?;
    let stored_payload: String = row.try_get("payload")?;
    tokio::task::spawn_blocking(move || {
        let stored: Content =
            serde_json::from_str(&stored_payload).map_err(|_| Error::Dependency)?;
        if stored != expected {
            return Err(Error::Dependency);
        }
        let mut project = project;
        stored.apply(&mut project);
        Ok(project)
    })
    .await?
}

/// Caller has already applied the public visibility rule to this core entity.
pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    project: ProjectCore,
) -> Result<ProjectView, Error> {
    let mut project = children(app, generation, project).await?;
    let (title, description) = crate::creation_metadata::project_text(
        app,
        generation,
        project.project_id,
        project.client,
        &project.content,
    )
    .await?;
    let mut proposals = Vec::with_capacity(project.proposals.len());
    for proposal in std::mem::take(&mut project.proposals) {
        proposals.push(
            crate::proposal_metadata::compose(
                app,
                generation,
                project.project_id,
                project.coordinator,
                project.client,
                proposal,
            )
            .await?,
        );
    }
    Ok(project.into_view(title, description, proposals))
}
