//! The wallet's private interface, answered without a wallet.
//!
//! The API asks a wallet for an account and for a signature over each
//! command. Here an account is a pass account, and what authorizes a command
//! is the session key that signs its transaction. A signing job therefore
//! succeeds, with a placeholder, exactly when the account has a live session.
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use protocol::{
    CreateSigningJobRequest, OperationId, PAYLOAD_VERSION, SigningJobStatus, SigningJobView,
    Sr25519Signature, UnsignedContractCallV1, WalletLifecycle, WalletView,
};

use crate::provider::Provider;
use crate::transport::Error;

pub(crate) fn router(provider: Provider) -> Router {
    Router::new()
        .route("/internal/system-wallet", get(system))
        .route("/internal/signing-jobs", post(sign))
        .route("/internal/signing-jobs/{operation_id}", get(job))
        .with_state(provider)
}

async fn system(State(provider): State<Provider>) -> Json<WalletView> {
    Json(WalletView {
        wallet_id: provider.system_wallet(),
        principal_id: None,
        account_id: provider.operator(),
        lifecycle: WalletLifecycle::Active,
    })
}

/// No job is kept: asking for one always leads to a new request.
async fn job(Path(_): Path<OperationId>) -> Response {
    Error {
        code: "not_found",
        status: StatusCode::NOT_FOUND,
    }
    .into_response()
}

async fn sign(
    State(provider): State<Provider>,
    Json(request): Json<CreateSigningJobRequest>,
) -> Result<Response, Error> {
    let call = UnsignedContractCallV1::decode_signable(&request.signable_payload)
        .map_err(|_| Error::bad("invalid_payload"))?;
    if request.payload_version != PAYLOAD_VERSION
        || call.operation_id != request.operation_id
        || call.expires_at != request.expires_at
    {
        return Err(Error::bad("invalid_payload"));
    }
    let live = provider
        .has_session(call.origin)
        .map_err(|_| Error::unavailable())?;
    let view = SigningJobView {
        operation_id: request.operation_id,
        wallet_id: request.wallet_id,
        account_id: call.origin,
        status: if live {
            SigningJobStatus::Signed
        } else {
            SigningJobStatus::Rejected
        },
        // The session key signs the transaction; nothing reads this value.
        signature: live.then(|| Sr25519Signature::from_bytes([0; 64])),
        rejection_code: (!live).then(|| "session_expired".into()),
    };
    Ok((StatusCode::ACCEPTED, Json(view)).into_response())
}
