//! Catalog requests remain provider-owned; adapter only scopes their reads.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
};
use generated_contracts::{AccountId32, SkillRequestView};

use crate::{
    auth::Session,
    state::{App, Error},
};

pub(crate) async fn mine(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<Vec<SkillRequestView>>, Error> {
    Ok(Json(read(&app, session.view.account_id).await?))
}

pub(crate) async fn all(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<Vec<SkillRequestView>>, Error> {
    if !session.view.is_admin {
        return Err(Error::Forbidden);
    }
    let root = app.snapshot().await?.info.root_account;
    Ok(Json(read(&app, root).await?))
}

async fn read(app: &App, account: AccountId32) -> Result<Vec<SkillRequestView>, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/catalog/skill-requests?account={account}"),
    )
    .await
}
