use crate::{
    auth::Session,
    profiles,
    profiles::models::PublicProfilesView,
    state::{App, Error, parse},
};
use axum::{
    Json,
    extract::{Extension, Path, State},
};
use generated_contracts::{AccountId32, EntityId, PrincipalId, ProposalStatus};
use serde::Serialize;
use sqlx::Row;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParticipantView {
    account_id: AccountId32,
    display_name: Option<String>,
    contact_email: Option<String>,
    profiles: Option<PublicProfilesView>,
    client_image: bool,
    worker_image: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectParticipantsView {
    client: ParticipantView,
    coordinator: ParticipantView,
    workers: Vec<ParticipantView>,
}

async fn participant(
    app: &App,
    account: AccountId32,
    is_client: bool,
) -> Result<ParticipantView, Error> {
    let row = sqlx::query("SELECT p.principal_id, p.display_name, c.contact_email AS client_contact_email, w.contact_email AS worker_contact_email, c.image_data IS NOT NULL AS client_image, w.image_data IS NOT NULL AS worker_image FROM principals p LEFT JOIN client_profiles c USING (principal_id) LEFT JOIN worker_profiles w USING (principal_id) WHERE p.account_id = $1")
        .bind(account.to_string()).fetch_optional(&app.db).await?;
    let (display_name, contact_email, public, client_image, worker_image) = if let Some(row) = row {
        let principal = parse::<PrincipalId>(&row.try_get::<String, _>("principal_id")?)?;
        (
            Some(row.try_get("display_name")?),
            row.try_get(if is_client {
                "client_contact_email"
            } else {
                "worker_contact_email"
            })?,
            Some(profiles::store::read(&app.db, principal).await?.into()),
            row.try_get("client_image")?,
            row.try_get("worker_image")?,
        )
    } else {
        (None, None, None, false, false)
    };
    Ok(ParticipantView {
        account_id: account,
        display_name,
        contact_email,
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
        .into_iter()
        .find(|p| p.project_id == id)
        .ok_or(Error::NotFound)?;
    let (project, _) = crate::task_views::project(
        &app,
        snapshot.info.provider_instance_id,
        project,
        session.view.account_id,
    )
    .await?;
    let project = project.ok_or(Error::NotFound)?;
    let (client, coordinator) = tokio::try_join!(
        participant(&app, project.client, true),
        participant(&app, project.coordinator, false)
    )?;
    let accounts: BTreeSet<_> = project
        .proposals
        .iter()
        .filter(|proposal| proposal.status == ProposalStatus::Approved)
        .flat_map(|proposal| &proposal.milestones)
        .flat_map(|milestone| &milestone.assignments)
        .map(|assignment| assignment.worker)
        .filter(|account| *account != project.client && *account != project.coordinator)
        .collect();
    let mut workers = Vec::with_capacity(accounts.len());
    for account in accounts {
        workers.push(participant(&app, account, false).await?);
    }
    Ok(Json(ProjectParticipantsView {
        client,
        coordinator,
        workers,
    }))
}
