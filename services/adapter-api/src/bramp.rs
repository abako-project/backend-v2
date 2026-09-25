//! Browser reads for the mock ramp; writes use the signed operation pipeline.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
};
use generated_contracts::{DepositView, EntityId, WithdrawalView};

use crate::{
    auth::Session,
    state::{App, Error, parse},
};

pub(crate) async fn deposit(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<DepositView>, Error> {
    Ok(Json(
        read_deposit(&app, session.view.account_id, parse(&id)?).await?,
    ))
}

pub(crate) async fn withdrawal(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<WithdrawalView>, Error> {
    Ok(Json(
        read_withdrawal(&app, session.view.account_id, parse(&id)?).await?,
    ))
}

pub(crate) async fn read_withdrawal(
    app: &App,
    account: generated_contracts::AccountId32,
    id: EntityId,
) -> Result<WithdrawalView, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/bramp/withdrawals/{id}?account={account}"),
    )
    .await
}

async fn read_deposit(
    app: &App,
    account: generated_contracts::AccountId32,
    id: EntityId,
) -> Result<DepositView, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/bramp/deposits/{id}?account={account}"),
    )
    .await
}
