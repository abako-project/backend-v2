use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::{Auth, Config, RetryConfig};
use crate::error::{self, ApiErrorDetail, Error};

#[derive(Serialize)]
struct ExchangeRequest {
    key: String,
    scopes: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ExchangeResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Clone)]
struct AuthState {
    auth: Auth,
    origin: Option<String>,
    urn: Option<String>,
    access_token: Option<String>,
    /// Unix millis when the current `access_token` (apiKey auth only) expires.
    exchange_expiry_ms: i64,
}

/// Thin `reqwest` wrapper replicating the wire behavior of the official
/// TypeScript SDK's internal `HttpClient`: base URL selection, auth header
/// construction, apiKey→JWT exchange with caching, idempotency keys on
/// mutating requests, and retry-with-backoff on transient failures.
pub(crate) struct HttpClient {
    client: reqwest::Client,
    base_url: String,
    retry: RetryConfig,
    timeout: Duration,
    state: RwLock<AuthState>,
    exchange_lock: tokio::sync::Mutex<()>,
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

impl HttpClient {
    pub(crate) fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let client = reqwest::Client::builder().build().map_err(Error::Network)?;
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| config.mode.base_url().to_string());
        // `Token` auth already carries a usable access token — load it up
        // front so `build_auth_header` picks it up immediately, with no
        // exchange step to run.
        let access_token = match &config.auth {
            Auth::Token { access_token } => Some(access_token.clone()),
            Auth::ApiKey { .. } | Auth::OriginKey { .. } => None,
        };
        Ok(Self {
            client,
            base_url,
            retry: config.retry.clone(),
            timeout: config.timeout,
            state: RwLock::new(AuthState {
                auth: config.auth,
                origin: config.origin,
                urn: None,
                access_token,
                exchange_expiry_ms: 0,
            }),
            exchange_lock: tokio::sync::Mutex::new(()),
        })
    }

    // The state is plain data and every write is a single field store, so a
    // poisoned lock cannot hold a half-updated value. Recover instead of panicking.
    fn read_state(&self) -> RwLockReadGuard<'_, AuthState> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_state(&self) -> RwLockWriteGuard<'_, AuthState> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Clone this client's config into a fresh, independent `HttpClient`.
    /// Used to give each enrolled user's [`crate::Session`] its own access
    /// token without disturbing the org-level client it was created from —
    /// mirrors the TS SDK's `httpClient.fork()`.
    pub(crate) fn fork(&self) -> Arc<HttpClient> {
        let state = self.read_state().clone();
        Arc::new(HttpClient {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            retry: self.retry.clone(),
            timeout: self.timeout,
            state: RwLock::new(state),
            exchange_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub(crate) fn origin(&self) -> Option<String> {
        self.read_state().origin.clone()
    }

    pub(crate) fn require_origin(&self) -> Result<String, Error> {
        self.origin()
            .ok_or_else(|| Error::Config("origin is required for this operation".into()))
    }

    pub(crate) fn urn(&self) -> Option<String> {
        self.read_state().urn.clone()
    }

    pub(crate) fn require_urn(&self) -> Result<String, Error> {
        self.urn().ok_or_else(|| {
            Error::Config("no identity URN on this session — connect() or register() first".into())
        })
    }

    pub(crate) fn set_urn(&self, urn: impl Into<String>) {
        self.write_state().urn = Some(urn.into());
    }

    pub(crate) fn set_origin(&self, origin: impl Into<String>) {
        self.write_state().origin = Some(origin.into());
    }

    pub(crate) fn set_access_token(&self, token: impl Into<String>) {
        self.write_state().access_token = Some(token.into());
    }

    pub(crate) fn access_token(&self) -> Option<String> {
        self.read_state().access_token.clone()
    }

    pub(crate) fn origin_key(&self) -> Option<String> {
        match &self.read_state().auth {
            Auth::OriginKey { origin_key } => Some(origin_key.clone()),
            Auth::ApiKey { .. } | Auth::Token { .. } => None,
        }
    }

    fn build_auth_header(&self) -> Option<String> {
        let state = self.read_state();
        if let Some(token) = &state.access_token {
            return Some(format!("Bearer {token}"));
        }
        match &state.auth {
            Auth::OriginKey { origin_key } => Some(origin_key.clone()),
            Auth::ApiKey { .. } | Auth::Token { .. } => None,
        }
    }

    /// Exchange the configured `sk_` key for a short-lived JWT, if we don't
    /// already hold a fresh one. No-op for `OriginKey` auth. Safe to call
    /// concurrently — only one exchange happens in flight at a time.
    async fn ensure_exchanged(&self) -> Result<(), Error> {
        let is_api_key = matches!(self.read_state().auth, Auth::ApiKey { .. });
        if !is_api_key {
            return Ok(());
        }

        let needs_exchange = |state: &AuthState| {
            state.access_token.is_none() || now_ms() >= state.exchange_expiry_ms - 60_000
        };

        if !needs_exchange(&self.read_state()) {
            return Ok(());
        }

        let _guard = self.exchange_lock.lock().await;
        if !needs_exchange(&self.read_state()) {
            return Ok(()); // someone else refreshed it while we waited
        }

        let (api_key, scopes) = {
            let state = self.read_state();
            match &state.auth {
                Auth::ApiKey { api_key, scopes } => (api_key.clone(), scopes.clone()),
                Auth::OriginKey { .. } | Auth::Token { .. } => unreachable!(),
            }
        };

        let body = serde_json::to_value(ExchangeRequest {
            key: api_key,
            scopes,
        })
        .map_err(|e| Error::Config(format!("failed to serialize exchange request: {e}")))?;
        let value = self
            .request_raw(Method::POST, "/api/api-keys/exchange", Some(body), None)
            .await?;
        let resp: ExchangeResponse = serde_json::from_value(value).map_err(|e| Error::Api {
            status: None,
            message: format!("failed to decode exchange response: {e}"),
            detail: ApiErrorDetail::default(),
        })?;

        let mut state = self.write_state();
        state.access_token = Some(resp.access_token);
        state.exchange_expiry_ms = now_ms() + resp.expires_in * 1000;
        Ok(())
    }

    fn calculate_retry_delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(ra) = retry_after {
            return ra.min(self.retry.max_delay);
        }
        let max_delay = self.retry.max_delay;
        // Clamp the exponent so `powi` cannot overflow; the result is capped
        // at `max_delay` anyway.
        let exponent = i32::try_from(attempt.min(30)).unwrap_or(30);
        let base = self.retry.initial_delay.as_secs_f64() * 2f64.powi(exponent);
        let jitter = base * 0.25 * (2.0 * rand::random::<f64>() - 1.0);
        Duration::try_from_secs_f64((base + jitter).max(0.0))
            .unwrap_or(max_delay)
            .min(max_delay)
    }

    /// Issue one API request, retrying on transient failures, and return the
    /// raw decoded JSON body. Deliberately **not generic** — it's called
    /// from [`Self::ensure_exchanged`], and a generic `async fn` that calls
    /// itself (even through another function, even at a different type)
    /// can't be sized by the compiler without boxing. Keeping the actual
    /// HTTP work here, non-generic, sidesteps that: [`Self::request`] and
    /// [`Self::ensure_exchanged`] both call this directly instead of each
    /// other.
    async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
        extra_headers: Option<reqwest::header::HeaderMap>,
    ) -> Result<serde_json::Value, Error> {
        let url = format!("{}{}", self.base_url, path);
        let max_attempts = if self.retry.enabled {
            self.retry.max_retries
        } else {
            0
        };
        let mut attempt = 0u32;

        loop {
            let mut req = self
                .client
                .request(method.clone(), &url)
                .header("Content-Type", "application/json")
                .header(
                    "X-Bloque-SDK",
                    concat!("bloque-rs@", env!("CARGO_PKG_VERSION")),
                )
                .timeout(self.timeout);

            if let Some(auth) = self.build_auth_header() {
                req = req.header("Authorization", auth);
            }
            if let Some(headers) = &extra_headers {
                req = req.headers(headers.clone());
            }
            if matches!(method, Method::POST | Method::PUT)
                && !extra_headers
                    .as_ref()
                    .is_some_and(|h| h.contains_key("idempotency-key"))
            {
                req = req.header("Idempotency-Key", uuid::Uuid::new_v4().to_string());
            }
            if let Some(b) = &body {
                req = req.json(b);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let request_id = resp
                        .headers()
                        .get("X-Request-ID")
                        .or_else(|| resp.headers().get("Request-ID"))
                        .and_then(|v| v.to_str().ok())
                        .map(ToString::to_string);
                    let retry_after = resp
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(Duration::from_secs);

                    let value: serde_json::Value =
                        resp.json().await.unwrap_or(serde_json::Value::Null);

                    if status.is_success() {
                        return Ok(value);
                    }

                    let err =
                        error::from_response(status.as_u16(), &value, request_id, retry_after);
                    if err.is_retryable() && attempt < max_attempts {
                        let delay = self.calculate_retry_delay(attempt, retry_after);
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
                Err(e) => {
                    let err = if e.is_timeout() {
                        Error::Timeout(self.timeout)
                    } else {
                        Error::Network(e)
                    };
                    if err.is_retryable() && attempt < max_attempts {
                        let delay = self.calculate_retry_delay(attempt, None);
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    /// Issue one API request, retrying on transient failures. `skip_exchange`
    /// avoids exchanging a fresh apiKey token before this call *is* the
    /// token exchange itself.
    pub(crate) async fn request<B, R>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        extra_headers: Option<reqwest::header::HeaderMap>,
        skip_exchange: bool,
    ) -> Result<R, Error>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        if !skip_exchange {
            self.ensure_exchanged().await?;
        }

        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| Error::Config(format!("failed to serialize request body: {e}")))?;

        let value = self.request_raw(method, path, body, extra_headers).await?;

        serde_json::from_value(value).map_err(|e| Error::Api {
            status: None,
            message: format!("failed to decode response: {e}"),
            detail: ApiErrorDetail::default(),
        })
    }
}
