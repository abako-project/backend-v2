use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use generated_contracts::{AccountId32, EntityId, OperationId, SignedContractCallV1, UnixSeconds};
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::{Error, Provider, Result};

#[derive(Clone)]
struct App {
    provider: Provider,
    authorization: Arc<str>,
    permits: Arc<Semaphore>,
}

/// Private HTTP router. Health is not privileged; all data and command routes
/// require the service credential and must never be exposed by the public proxy.
pub fn router(provider: Provider, service_token: &str) -> Result<Router> {
    if service_token.trim().len() < 32 || service_token.contains(['\r', '\n']) {
        return Err(Error::bad("invalid_service_token"));
    }
    let state = App {
        provider,
        authorization: format!("Bearer {service_token}").into(),
        permits: Arc::new(Semaphore::new(64)),
    };
    let private = Router::new()
        .route("/internal/info", get(info))
        .route("/internal/accounts/{account}/nonce", get(nonce))
        .route("/internal/contracts/call", post(call))
        .route("/internal/receipts/{operation_id}", get(receipt))
        .route("/internal/events", get(events))
        .route("/internal/snapshot", get(snapshot))
        .route("/internal/catalog/skill-requests", get(skill_requests))
        .route("/internal/disputes/{dispute_id}", get(dispute))
        .route("/internal/bramp/deposits/{deposit_id}", get(deposit))
        .route(
            "/internal/bramp/withdrawals/{withdrawal_id}",
            get(withdrawal),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Ok(private
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/ready", get(ready))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state))
}

async fn authenticate(State(state): State<App>, request: Request, next: Next) -> Response {
    if request
        .headers()
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        != Some(state.authorization.as_ref())
    {
        return Error {
            code: "unauthorized_service",
            status: StatusCode::UNAUTHORIZED,
        }
        .into_response();
    }
    let Ok(_permit) = state.permits.try_acquire() else {
        return Error {
            code: "provider_busy",
            status: StatusCode::TOO_MANY_REQUESTS,
        }
        .into_response();
    };
    // A timeout is an unknown transport outcome. Durable receipts permit recovery.
    tokio::time::timeout(Duration::from_secs(10), next.run(request))
        .await
        .unwrap_or_else(|_| Error::internal().into_response())
}

async fn ready(State(state): State<App>) -> Result<StatusCode> {
    state.provider.info().await?;
    Ok(StatusCode::OK)
}
async fn info(State(state): State<App>) -> Result<impl IntoResponse> {
    Ok(Json(state.provider.info().await?))
}
async fn snapshot(State(state): State<App>) -> Result<impl IntoResponse> {
    Ok(Json(state.provider.snapshot().await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillRequestsQuery {
    account: AccountId32,
}

async fn skill_requests(
    State(state): State<App>,
    query: std::result::Result<Query<SkillRequestsQuery>, QueryRejection>,
) -> Result<impl IntoResponse> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_skill_requests_query"))?;
    Ok(Json(state.provider.skill_requests(query.account).await?))
}

async fn dispute(
    State(state): State<App>,
    Path(id): Path<generated_contracts::EntityId>,
) -> Result<impl IntoResponse> {
    state.provider.dispute(id).await.map(Json).map_err(|error| {
        if error.code() == "dispute_not_found" {
            Error {
                code: "dispute_not_found",
                status: StatusCode::NOT_FOUND,
            }
        } else {
            error
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrampReadQuery {
    account: AccountId32,
}

async fn deposit(
    State(state): State<App>,
    Path(id): Path<EntityId>,
    Query(query): Query<BrampReadQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        state
            .provider
            .bramp_deposit(query.account, id)
            .await
            .map_err(bramp_read_error)?,
    ))
}

async fn withdrawal(
    State(state): State<App>,
    Path(id): Path<EntityId>,
    Query(query): Query<BrampReadQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        state
            .provider
            .bramp_withdrawal(query.account, id)
            .await
            .map_err(bramp_read_error)?,
    ))
}

fn bramp_read_error(error: Error) -> Error {
    match error.code {
        "deposit_not_found"
        | "deposit_owner_or_system_required"
        | "withdrawal_not_found"
        | "withdrawal_owner_or_system_required" => Error {
            code: "bramp_request_not_found",
            status: StatusCode::NOT_FOUND,
        },
        _ => error,
    }
}
async fn nonce(
    State(state): State<App>,
    path: std::result::Result<Path<AccountId32>, PathRejection>,
) -> Result<impl IntoResponse> {
    let Path(account) = path.map_err(|_| Error::bad("invalid_account_id"))?;
    Ok(Json(state.provider.nonce(account).await?))
}
async fn receipt(
    State(state): State<App>,
    path: std::result::Result<Path<OperationId>, PathRejection>,
) -> Result<impl IntoResponse> {
    let Path(operation) = path.map_err(|_| Error::bad("invalid_operation_id"))?;
    state
        .provider
        .receipt(operation)
        .await?
        .map(Json)
        .ok_or(Error {
            code: "receipt_not_found",
            status: StatusCode::NOT_FOUND,
        })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "page_size")]
    limit: usize,
}
fn page_size() -> usize {
    100
}
async fn events(
    State(state): State<App>,
    query: std::result::Result<Query<EventsQuery>, QueryRejection>,
) -> Result<impl IntoResponse> {
    let Query(query) = query.map_err(|_| Error::bad("invalid_events_query"))?;
    Ok(Json(state.provider.events(query.after, query.limit).await?))
}
async fn call(
    State(state): State<App>,
    call: std::result::Result<Json<SignedContractCallV1>, JsonRejection>,
) -> Result<impl IntoResponse> {
    let Json(call) = call.map_err(|_| Error::bad("invalid_contract_json"))?;
    let now = UnixSeconds::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::internal())?
            .as_secs(),
    );
    let result = state.provider.execute(call, now).await;
    match &result {
        Ok(receipt) => match &receipt.outcome {
            generated_contracts::ExecutionOutcome::Success => {
                tracing::info!(operation_id = %receipt.operation_id,
                entity_id = ?receipt.created_entity_id, cursor = ?receipt.last_event_cursor, "provider command committed");
            }
            generated_contracts::ExecutionOutcome::Failed(code) => {
                tracing::warn!(operation_id = %receipt.operation_id,
                code, "provider command rejected");
            }
        },
        Err(error) if error.status == StatusCode::SERVICE_UNAVAILABLE => {
            tracing::error!(code = error.code(), "provider unavailable");
        }
        Err(error) => tracing::warn!(code = error.code(), "provider envelope rejected"),
    }
    Ok(Json(result?))
}
