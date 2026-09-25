use std::time::Duration;

/// Shorthand for this crate's fallible return type. Every public function
/// here fails with the same [`Error`], so `bloque::Result<Account>` reads
/// the same as `std::result::Result<Account, bloque::Error>` without
/// repeating the error type at every call site.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to the Bloque API.
///
/// Mirrors the error hierarchy of the official TypeScript SDK
/// (`BloqueValidationError`, `BloqueAuthenticationError`, ...) so callers can
/// match on the same distinctions the API actually makes (via HTTP status
/// and the `code` field in the error body).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Bad SDK configuration (missing origin, wrong auth for the operation, ...).
    /// Never comes from the network.
    #[error("configuration error: {0}")]
    Config(String),

    /// HTTP 400 with no recognized `code`.
    #[error("validation error: {message}")]
    Validation {
        message: String,
        detail: ApiErrorDetail,
    },

    /// HTTP 401/403.
    #[error("authentication error: {message}")]
    Authentication {
        message: String,
        detail: ApiErrorDetail,
    },

    /// HTTP 404.
    #[error("not found: {message}")]
    NotFound {
        message: String,
        detail: ApiErrorDetail,
    },

    /// HTTP 429, or a 503 the server flagged as retryable-but-exhausted.
    #[error("rate limited: {message}")]
    RateLimit {
        message: String,
        retry_after: Option<Duration>,
        detail: ApiErrorDetail,
    },

    /// `code == "INSUFFICIENT_FUNDS" | "INSUFFICIENT_BALANCE"`.
    #[error("insufficient funds: {message}")]
    InsufficientFunds {
        message: String,
        detail: ApiErrorDetail,
    },

    /// `code == "E_VERIFICATION_REQUIRED"` — the identity needs KYC/TOS/docs
    /// before this operation can proceed.
    #[error("verification required: {message}")]
    VerificationRequired {
        message: String,
        detail: ApiErrorDetail,
    },

    /// `code == "E_VERIFICATION_PENDING"` — a submission is under review.
    #[error("verification pending: {message}")]
    VerificationPending {
        message: String,
        detail: ApiErrorDetail,
    },

    /// Any other non-2xx response.
    #[error("api error ({status:?}): {message}")]
    Api {
        status: Option<u16>,
        message: String,
        detail: ApiErrorDetail,
    },

    /// The request never got a response (DNS, TLS, connection reset, ...).
    #[error("network error: {0}")]
    Network(#[source] reqwest::Error),

    /// The request exceeded its configured timeout.
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
}

/// The raw bits of an API error response, kept around for callers that need
/// more than the human-readable message (structured logging, retry
/// decisions, surfacing `code` to a caller-side switch, ...).
#[derive(Debug, Clone, Default)]
pub struct ApiErrorDetail {
    pub status: Option<u16>,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub body: Option<serde_json::Value>,
}

impl Error {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::RateLimit { .. } | Error::Network(_) | Error::Timeout(_)
        ) || matches!(
            self,
            Error::Api {
                status: Some(503),
                ..
            }
        )
    }
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct ApiErrorBody {
    pub message: Option<String>,
    pub code: Option<String>,
}

/// Build the right `Error` variant from a non-2xx HTTP response, the way the
/// TS SDK's `createBloqueError` does.
pub(crate) fn from_response(
    status: u16,
    body: &serde_json::Value,
    request_id: Option<String>,
    retry_after: Option<Duration>,
) -> Error {
    let parsed: ApiErrorBody = serde_json::from_value(body.clone()).unwrap_or_default();
    let code = parsed.code;
    let message = parsed.message.unwrap_or_else(|| format!("HTTP {status}"));
    let detail = ApiErrorDetail {
        status: Some(status),
        code: code.clone(),
        request_id,
        body: Some(body.clone()),
    };

    match code.as_deref() {
        Some("E_VERIFICATION_REQUIRED") => return Error::VerificationRequired { message, detail },
        Some("E_VERIFICATION_PENDING") => return Error::VerificationPending { message, detail },
        Some("INSUFFICIENT_FUNDS" | "INSUFFICIENT_BALANCE") => {
            return Error::InsufficientFunds { message, detail };
        }
        _ => {}
    }

    match status {
        400 => Error::Validation { message, detail },
        401 | 403 => Error::Authentication { message, detail },
        404 => Error::NotFound { message, detail },
        429 => Error::RateLimit {
            message,
            retry_after,
            detail,
        },
        _ => Error::Api {
            status: Some(status),
            message,
            detail,
        },
    }
}
