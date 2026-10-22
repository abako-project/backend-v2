//! Disposable smart-contract simulator with one atomic command boundary.
#![allow(clippy::missing_errors_doc)]

mod calendar;
mod domain;
mod http;
mod provider_api;
mod reference;
mod seed;
mod storage;

pub use http::{router, router_with_provider};
pub use provider_api::ProviderApi;
pub use reference::ReferenceState;
pub use storage::Provider;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use generated_contracts::ApiError;

/// Sanitized provider failures. Domain rejection is recorded in a receipt;
/// invalid envelopes and infrastructure failures have no committed execution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct Error {
    code: &'static str,
    status: StatusCode,
}

impl Error {
    pub(crate) const fn domain(code: &'static str) -> Self {
        Self {
            code,
            status: StatusCode::CONFLICT,
        }
    }

    pub(crate) const fn bad(code: &'static str) -> Self {
        Self {
            code,
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub(crate) const fn internal() -> Self {
        Self {
            code: "provider_unavailable",
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Sanitized failure for a provider effect boundary.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self::internal()
    }
    #[must_use]
    /// Whether the failure represents an unavailable provider effect.
    pub fn is_infrastructure(&self) -> bool {
        self.status == StatusCode::SERVICE_UNAVAILABLE
    }

    /// Stable machine-readable reason, without request content or secrets.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl From<provider_execution::Error> for Error {
    fn from(error: provider_execution::Error) -> Self {
        if error.is_infrastructure() {
            Self::internal()
        } else if error.code() == "invalid_contract" {
            Self::bad(error.code())
        } else {
            Self::domain(error.code())
        }
    }
}
impl From<generated_contracts::ValidationError> for Error {
    fn from(_: generated_contracts::ValidationError) -> Self {
        Self::domain("arithmetic_or_calendar_error")
    }
}
impl From<generated_contracts::ContractError> for Error {
    fn from(_: generated_contracts::ContractError) -> Self {
        Self::bad("invalid_contract")
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                code: self.code.into(),
                message: self.code.replace('_', " "),
            }),
        )
            .into_response()
    }
}

/// Sanitized provider domain or infrastructure failure.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn require(condition: bool, code: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::domain(code))
    }
}
