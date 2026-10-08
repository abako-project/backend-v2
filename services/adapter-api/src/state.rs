use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use generated_contracts::{ApiError, PayloadHash, ProviderSnapshot};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeSet,
    env, fs,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid request")]
    Invalid,
    #[error("authentication required")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("resource not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(&'static str),
    #[error("capacity exceeded")]
    Capacity,
    #[error("dependency unavailable")]
    Dependency,
    #[error("invalid runtime configuration")]
    Config,
    #[error("internal error")]
    Internal,
    #[error("database unavailable")]
    Database(#[from] sqlx::Error),
    #[error("I/O failure")]
    Io(#[from] std::io::Error),
    #[error("background task failure")]
    Join(#[from] tokio::task::JoinError),
    #[error("invalid stored contract")]
    Json(#[from] serde_json::Error),
}
impl Error {
    pub(crate) const fn status_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::Invalid => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(code) => (StatusCode::CONFLICT, code),
            Self::Capacity => (StatusCode::TOO_MANY_REQUESTS, "capacity_exceeded"),
            Self::Dependency => (StatusCode::SERVICE_UNAVAILABLE, "dependency_unavailable"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = self.status_code();
        public_error(status, code)
    }
}
pub(crate) fn public_error(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(ApiError {
            code: code.to_owned(),
            message: status
                .canonical_reason()
                .unwrap_or("Request failed")
                .to_owned(),
        }),
    )
        .into_response()
}
pub(crate) struct Config {
    pub(crate) bind_addr: String,
    pub(crate) database_url: String,
    pub(crate) custody_url: String,
    pub(crate) provider_url: String,
    pub(crate) service_token: Zeroizing<String>,
    pub(crate) allowed_origins: BTreeSet<String>,
    pub(crate) cookie_secure: bool,
    pub(crate) enable_mock_funding: bool,
    pub(crate) admin_username: String,
    pub(crate) admin_password: Zeroizing<String>,
    pub(crate) openapi: String,
}
impl Config {
    pub(crate) fn from_env() -> Result<Self, Error> {
        let required_secret = |name| -> Result<Zeroizing<String>, Error> {
            let path = env::var(name).map_err(|_| Error::Config)?;
            let contents = Zeroizing::new(fs::read_to_string(path)?);
            let secret = Zeroizing::new(contents.trim().to_owned());
            if secret.len() < 12 {
                return Err(Error::Config);
            }
            Ok(secret)
        };
        let origins: BTreeSet<String> = env::var("ALLOWED_ORIGINS")
            .map_err(|_| Error::Config)?
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect();
        for origin in &origins {
            let parsed = reqwest::Url::parse(origin).map_err(|_| Error::Config)?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.origin().ascii_serialization() != *origin
            {
                return Err(Error::Config);
            }
        }
        let internal_url = |name: &str| -> Result<String, Error> {
            let value = env::var(name).map_err(|_| Error::Config)?;
            let parsed = reqwest::Url::parse(&value).map_err(|_| Error::Config)?;
            if parsed.scheme() != "http"
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
                || parsed.path() != "/"
            {
                return Err(Error::Config);
            }
            Ok(value.trim_end_matches('/').to_owned())
        };
        let openapi = fs::read_to_string(
            env::var("OPENAPI_PATH").unwrap_or_else(|_| "contracts/openapi.json".into()),
        )?;
        serde_json::from_str::<serde_json::Value>(&openapi)?;
        let cookie_secure = match env::var("COOKIE_SECURE").as_deref() {
            Ok("false") => false,
            Ok("true") | Err(_) => true,
            _ => return Err(Error::Config),
        };
        let enable_mock_funding = match env::var("ENABLE_MOCK_FUNDING").as_deref() {
            Ok("true") => true,
            Ok("false") | Err(_) => false,
            _ => return Err(Error::Config),
        };
        let service_token = required_secret("INTERNAL_SERVICE_TOKEN_FILE")?;
        if service_token.len() < 32 {
            return Err(Error::Config);
        }
        Ok(Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: env::var("ADAPTER_DATABASE_URL").unwrap_or_else(|_| {
                "postgres://kunveno:kunveno-local-only@postgres:5432/kunveno_adapter".into()
            }),
            custody_url: internal_url("CUSTODY_URL")?,
            provider_url: internal_url("MOCK_PROVIDER_URL")?,
            service_token,
            allowed_origins: origins,
            cookie_secure,
            enable_mock_funding,
            admin_username: env::var("BOOTSTRAP_ADMIN_USERNAME").unwrap_or_else(|_| "admin".into()),
            admin_password: required_secret("BOOTSTRAP_ADMIN_PASSWORD_FILE")?,
            openapi,
        })
    }
}
pub(crate) struct App {
    pub(crate) config: Config,
    pub(crate) db: PgPool,
    pub(crate) client: reqwest::Client,
    pub(crate) password_slots: Arc<Semaphore>,
    pub(crate) request_slots: Semaphore,
    pub(crate) stream_slots: Arc<Semaphore>,
}
impl App {
    pub(crate) async fn new(config: Config) -> Result<Self, Error> {
        crate::auth::passkeys::configured_webauthn()?;
        let db = PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.database_url)
            .await?;
        let mut migration = db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(621006)")
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0001_adapter.sql"))
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0002_profiles.sql"))
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0003_passkeys.sql"))
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0004_profile_images.sql"))
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0005_worker_contact_email.sql"))
            .execute(&mut *migration)
            .await?;
        sqlx::raw_sql(include_str!("../migrations/0006_project_briefs.sql"))
            .execute(&mut *migration)
            .await?;
        migration.commit().await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Config)?;
        Ok(Self {
            config,
            db,
            client,
            password_slots: Arc::new(Semaphore::new(4)),
            request_slots: Semaphore::new(128),
            stream_slots: Arc::new(Semaphore::new(256)),
        })
    }
    pub(crate) fn internal(
        &self,
        method: reqwest::Method,
        base: &str,
        path: &str,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{base}{path}"))
            .bearer_auth(self.config.service_token.as_str())
    }
    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        base: &str,
        path: &str,
    ) -> Result<T, Error> {
        let response = self
            .internal(reqwest::Method::GET, base, path)
            .send()
            .await
            .map_err(|_| Error::Dependency)?;
        decode_response(response).await
    }
    pub(crate) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        base: &str,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let response = self
            .internal(reqwest::Method::POST, base, path)
            .json(body)
            .send()
            .await
            .map_err(|_| Error::Dependency)?;
        decode_response(response).await
    }
    pub(crate) async fn snapshot(&self) -> Result<ProviderSnapshot, Error> {
        self.get(&self.config.provider_url, "/internal/snapshot")
            .await
    }
    pub(crate) async fn audit(
        &self,
        principal: Option<&str>,
        operation: Option<&str>,
        kind: &str,
        code: &str,
    ) -> Result<(), Error> {
        sqlx::query("INSERT INTO audit_records(principal_id, operation_id, event_kind, result_code, occurred_at) VALUES ($1, $2, $3, $4, $5)")
            .bind(principal).bind(operation).bind(kind).bind(code).bind(now()?).execute(&self.db).await?;
        Ok(())
    }
}
pub(crate) async fn decode_response<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, Error> {
    const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(Error::NotFound);
    }
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "internal service returned an unsuccessful response");
        return Err(Error::Dependency);
    }
    // ponytail: bounded whole snapshot for this POC; paginate provider reads as data grows.
    if response
        .content_length()
        .is_some_and(|size| size > RESPONSE_LIMIT as u64)
    {
        return Err(Error::Dependency);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Dependency)? {
        if chunk.len() > RESPONSE_LIMIT.saturating_sub(bytes.len()) {
            return Err(Error::Dependency);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Dependency)
}
pub(crate) fn now() -> Result<i64, Error> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Internal)?
            .as_secs(),
    )
    .map_err(|_| Error::Internal)
}
pub(crate) fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Internal)?;
    Ok(bytes)
}
pub(crate) fn token() -> Result<Zeroizing<String>, Error> {
    Ok(Zeroizing::new(
        generated_contracts::AccountId32::from_bytes(random()?).to_string(),
    ))
}
pub(crate) fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub(crate) fn payload_hash(bytes: &[u8]) -> PayloadHash {
    PayloadHash::from_bytes(hash(bytes))
}
pub(crate) fn parse<T: FromStr>(value: &str) -> Result<T, Error> {
    value.parse().map_err(|_| Error::Invalid)
}
