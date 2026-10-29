use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use protocol::ApiError;

/// Sanitized provider failures. Domain rejection is recorded in a receipt;
/// invalid envelopes and infrastructure failures have no committed execution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct Error {
    pub(crate) code: &'static str,
    pub(crate) status: StatusCode,
}

impl Error {
    /// Domain read rejection; commands record business failures in their receipts.
    #[must_use]
    pub const fn domain(code: &'static str) -> Self {
        Self {
            code,
            status: StatusCode::CONFLICT,
        }
    }

    /// Infrastructure failure with a stable, sanitized reason.
    #[must_use]
    pub const fn infrastructure(code: &'static str) -> Self {
        Self {
            code,
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Invalid private transport request.
    #[must_use]
    pub const fn bad(code: &'static str) -> Self {
        Self {
            code,
            status: StatusCode::BAD_REQUEST,
        }
    }

    /// Infrastructure failure without exposing internal details.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self::infrastructure("provider_unavailable")
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

impl From<super::execution::Error> for Error {
    fn from(error: super::execution::Error) -> Self {
        if error.is_infrastructure() {
            Self::unavailable()
        } else if error.code() == "invalid_contract" {
            Self::bad(error.code())
        } else {
            Self::domain(error.code())
        }
    }
}
impl From<protocol::ValidationError> for Error {
    fn from(_: protocol::ValidationError) -> Self {
        Self::domain("arithmetic_or_calendar_error")
    }
}
impl From<protocol::ContractError> for Error {
    fn from(_: protocol::ContractError) -> Self {
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
