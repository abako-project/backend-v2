use crate::store::{Error, Store};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use generated_contracts::{
    ApiError, CreateSigningJobRequest, OperationId, ProvisionWalletRequest, SigningJobView,
    WalletView,
};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct AppState {
    store: Store,
    token_hash: [u8; 32],
    permits: Arc<Semaphore>,
}

impl AppState {
    pub(crate) fn new(store: Store, token: Zeroizing<String>) -> Self {
        let token_hash = Sha256::digest(token.as_bytes()).into();
        drop(token);
        Self {
            store,
            token_hash,
            permits: Arc::new(Semaphore::new(64)),
        }
    }
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/internal/wallets", post(provision))
        .route("/internal/system-wallet", get(system))
        .route("/internal/signing-jobs", post(create_job))
        .route("/internal/signing-jobs/{operation_id}", get(job))
        .route("/internal/metrics", get(metrics))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route("/ready", get(ready))
        .layer(DefaultBodyLimit::max(
            generated_contracts::MAX_SIGNABLE_BYTES * 4 + 4096,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), bound_request))
        .with_state(state)
}

async fn authenticate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    // Compare hashes rather than raw secrets, avoiding a token-prefix timing oracle.
    if supplied.map(|token| <[u8; 32]>::from(Sha256::digest(token.as_bytes())))
        != Some(state.token_hash)
    {
        state.store.unauthorized().await?;
        return Err(Error::Unauthorized);
    }
    Ok(next.run(request).await)
}

async fn bound_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let _permit = state.permits.try_acquire().map_err(|_| Error::QueueFull)?;
    let mut response = tokio::time::timeout(Duration::from_secs(5), next.run(request))
        .await
        .map_err(|_| Error::Worker)?;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn ready(State(state): State<AppState>) -> Result<StatusCode, Error> {
    state.store.ready().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn metrics(State(state): State<AppState>) -> Result<String, Error> {
    state.store.metrics().await
}

async fn provision(
    State(state): State<AppState>,
    body: Result<Json<ProvisionWalletRequest>, JsonRejection>,
) -> Result<Json<WalletView>, Error> {
    let Json(request) = body.map_err(|_| Error::InvalidRequest)?;
    state.store.provision(request.principal_id).await.map(Json)
}

async fn system(State(state): State<AppState>) -> Result<Json<WalletView>, Error> {
    state.store.system_wallet().await.map(Json)
}

async fn create_job(
    State(state): State<AppState>,
    body: Result<Json<CreateSigningJobRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<SigningJobView>), Error> {
    let Json(request) = body.map_err(|_| Error::InvalidRequest)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(state.store.create_job(request).await?),
    ))
}

async fn job(
    State(state): State<AppState>,
    Path(operation): Path<OperationId>,
) -> Result<Json<SigningJobView>, Error> {
    state.store.job(operation).await.map(Json)
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::InvalidRequest | Self::Expired => StatusCode::BAD_REQUEST,
            Self::Conflict | Self::Inactive => StatusCode::CONFLICT,
            Self::QueueFull => StatusCode::TOO_MANY_REQUESTS,
            Self::Configuration
            | Self::Entropy
            | Self::KeyUnavailable
            | Self::Database(_)
            | Self::Worker
            | Self::Clock => StatusCode::SERVICE_UNAVAILABLE,
        };
        let code = self.to_string();
        (
            status,
            Json(ApiError {
                message: code.clone(),
                code,
            }),
        )
            .into_response()
    }
}
