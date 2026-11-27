use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use protocol::{ApiError, PayloadHash, provider_core::CoreSnapshot};
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
    #[error(
        "unsupported adapter database schema; use an empty database for this development build"
    )]
    Schema,
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
    pub(crate) dispute_channel_allow_participants: bool,
    pub(crate) admin_username: String,
    pub(crate) admin_password: Zeroizing<String>,
    pub(crate) openapi: String,
    pub(crate) platform: protocol::PlatformProfile,
    /// Set when the contracts run on Kreivo instead of the provider service.
    pub(crate) chain: Option<ChainConfig>,
}
/// The contracts on a fork of Kreivo, in this process.
pub(crate) struct ChainConfig {
    /// The snapshot of Kreivo the first start forks.
    pub(crate) snapshot: std::path::PathBuf,
    /// Where the fork and its state are saved.
    pub(crate) data_dir: std::path::PathBuf,
    /// The directory of the Dao and Project bundles.
    pub(crate) contracts_dir: std::path::PathBuf,
    /// The secret every device key is derived with.
    pub(crate) derivation_secret: Zeroizing<String>,
    /// The ID of a community to set up on the fork and make every account a
    /// member of, so that its usage subscription pays their fees.
    pub(crate) community: Option<u16>,
}
impl ChainConfig {
    /// The chain's settings, if `KREIVO_URL` asks for one.
    fn from_env(
        secret: &impl Fn(&'static str) -> Result<Zeroizing<String>, Error>,
    ) -> Result<Option<Self>, Error> {
        let url = match env::var("KREIVO_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => return Ok(None),
        };
        let derivation_secret = secret("DERIVATION_SECRET_FILE")?;
        if derivation_secret.len() < 32 {
            return Err(Error::Config);
        }
        let directory = |name| env::var_os(name).map(Into::into).ok_or(Error::Config);
        Ok(Some(Self {
            snapshot: url.strip_prefix("fork:").ok_or(Error::Config)?.into(),
            data_dir: directory("KREIVO_DATA_DIR")?,
            contracts_dir: directory("KREIVO_CONTRACTS_DIR")?,
            derivation_secret,
            community: match env::var("KREIVO_COMMUNITY") {
                Ok(id) => Some(id.parse().map_err(|_| Error::Config)?),
                Err(_) => None,
            },
        }))
    }
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
        let dispute_channel_allow_participants =
            match env::var("DISPUTE_CHANNEL_ALLOW_PARTICIPANTS").as_deref() {
                Ok("true") => true,
                Ok("false") | Err(_) => false,
                _ => return Err(Error::Config),
            };
        let chain = ChainConfig::from_env(&required_secret)?;
        // With the chain in this process there are no private services to reach.
        let (custody_url, provider_url, service_token) = if chain.is_some() {
            (String::new(), String::new(), Zeroizing::new(String::new()))
        } else {
            let service_token = required_secret("INTERNAL_SERVICE_TOKEN_FILE")?;
            if service_token.len() < 32 {
                return Err(Error::Config);
            }
            (
                internal_url("CUSTODY_URL")?,
                internal_url("MOCK_PROVIDER_URL")?,
                service_token,
            )
        };
        Ok(Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: env::var("ADAPTER_DATABASE_URL").unwrap_or_else(|_| {
                "postgres://digesta:digesta-local-only@postgres:5432/digesta_adapter".into()
            }),
            custody_url,
            provider_url,
            service_token,
            allowed_origins: origins,
            cookie_secure,
            enable_mock_funding,
            dispute_channel_allow_participants,
            admin_username: env::var("BOOTSTRAP_ADMIN_USERNAME").unwrap_or_else(|_| "admin".into()),
            admin_password: required_secret("BOOTSTRAP_ADMIN_PASSWORD_FILE")?,
            openapi,
            platform: crate::platform::load(
                env::var_os("PLATFORM_CONFIG_PATH")
                    .as_deref()
                    .map(std::path::Path::new),
            )?,
            chain,
        })
    }
}
pub(crate) struct App {
    pub(crate) config: Config,
    pub(crate) db: PgPool,
    pub(crate) client: reqwest::Client,
    /// The contracts on Kreivo, when they are not behind the provider service.
    pub(crate) chain: Option<chain_provider::Provider>,
    /// The passkey ceremonies under way on the chain.
    pub(crate) ceremonies:
        std::sync::Mutex<std::collections::HashMap<String, crate::auth::passkeys::chain::Ceremony>>,
    /// Held while a password change is on its way to the chain, and while
    /// one whose answer was lost is settled.
    pub(crate) credentials: Arc<tokio::sync::Mutex<()>>,
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
        let mut setup = db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(621006)")
            .execute(&mut *setup)
            .await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=current_schema() AND table_name='adapter_schema')")
            .fetch_one(&mut *setup).await?;
        if exists {
            let formats: Vec<String> = sqlx::query_scalar("SELECT format FROM adapter_schema")
                .fetch_all(&mut *setup)
                .await?;
            if formats != ["current"] {
                return Err(Error::Schema);
            }
        } else {
            let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=current_schema())")
                .fetch_one(&mut *setup).await?;
            if occupied {
                return Err(Error::Schema);
            }
            sqlx::raw_sql(include_str!("../schema.sql"))
                .execute(&mut *setup)
                .await?;
        }
        setup.commit().await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Config)?;
        let chain = match &config.chain {
            Some(chain) => {
                crate::auth::passkeys::chain::check_relying_party()?;
                // The database and the saved fork describe one marketplace: a
                // new one beside an old other is refused before anything is
                // written, not repaired. A fork is new until its first start
                // is over, and until then only the administrator exists.
                let resumed = chain_provider::Provider::saved(&chain.data_dir);
                // Only a record this mode wrote is written again: an
                // administrator of the service mode is left alone and refused.
                let devices = format!("{}%", crate::auth::DEVICE_PREFIX);
                let (principals, administrators, unfinished): (i64, i64, i64) = sqlx::query_as(
                    "SELECT COUNT(*), COUNT(*) FILTER (WHERE username = $1 AND is_admin), COUNT(*) FILTER (WHERE is_admin AND account_id IS NULL AND password_hash LIKE $2) FROM principals",
                )
                .bind(config.admin_username.to_ascii_lowercase())
                .bind(&devices)
                .fetch_one(&db)
                .await?;
                // A saved fork has its administrator here. A new fork has
                // nobody, or the administrator of a first start that did not
                // finish, who is written again.
                let leftover = principals == 1 && unfinished == 1;
                if (resumed && administrators != 1) || (!resumed && principals > 0 && !leftover) {
                    tracing::error!(
                        resumed,
                        "the database and the saved fork do not belong together"
                    );
                    return Err(Error::Config);
                }
                if !resumed && leftover {
                    sqlx::query("DELETE FROM principals WHERE is_admin AND account_id IS NULL AND password_hash LIKE $1")
                        .bind(&devices)
                        .execute(&db)
                        .await?;
                }
                // The administrator's pass account is the Dao's operator.
                let (device, _) = crate::auth::bootstrap_device(&db, &config).await?;
                // A session's key lives in this process: none survives a restart.
                sqlx::query("DELETE FROM sessions").execute(&db).await?;
                let provider = chain_provider::Provider::open(
                    chain_provider::Config {
                        snapshot: chain.snapshot.clone(),
                        data_dir: chain.data_dir.clone(),
                        contracts_dir: chain.contracts_dir.clone(),
                        mock_funding: config.enable_mock_funding,
                        operator_user: config.admin_username.to_ascii_lowercase(),
                        community: chain.community,
                    },
                    device.as_ref(),
                )
                .await
                .map_err(|error| {
                    tracing::error!(%error, "the chain could not be started");
                    Error::Config
                })?;
                Some(provider)
            }
            None => None,
        };
        Ok(Self {
            config,
            db,
            client,
            chain,
            ceremonies: std::sync::Mutex::default(),
            credentials: Arc::default(),
            password_slots: Arc::new(Semaphore::new(4)),
            request_slots: Semaphore::new(128),
            stream_slots: Arc::new(Semaphore::new(256)),
        })
    }
    /// Send a request to a private service, or answer it from the chain in
    /// this process when there is one.
    pub(crate) async fn send(
        &self,
        method: reqwest::Method,
        base: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<InternalResponse, Error> {
        if let Some(chain) = &self.chain {
            use tower::ServiceExt;
            let router = chain.router().map_err(|_| Error::Dependency)?;
            let request = axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header(
                    axum::http::header::AUTHORIZATION,
                    format!("Bearer {}", chain.token()),
                )
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(body.unwrap_or_default()))
                .map_err(|_| Error::Internal)?;
            let response = router
                .oneshot(request)
                .await
                .map_err(|_| Error::Dependency)?;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), RESPONSE_LIMIT)
                .await
                .map_err(|_| Error::Dependency)?
                .to_vec();
            return Ok(InternalResponse { status, body });
        }
        let mut request = self
            .client
            .request(method, format!("{base}{path}"))
            .bearer_auth(self.config.service_token.as_str());
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let response = request.send().await.map_err(|_| Error::Dependency)?;
        let status = response.status();
        let body = if status.is_success() {
            bounded_response(response).await?
        } else {
            Vec::new()
        };
        Ok(InternalResponse { status, body })
    }
    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        base: &str,
        path: &str,
    ) -> Result<T, Error> {
        self.send(reqwest::Method::GET, base, path, None)
            .await?
            .decode()
    }
    pub(crate) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        base: &str,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        self.send(
            reqwest::Method::POST,
            base,
            path,
            Some(serde_json::to_vec(body)?),
        )
        .await?
        .decode()
    }
    pub(crate) async fn snapshot(&self) -> Result<CoreSnapshot, Error> {
        let snapshot: CoreSnapshot = self
            .get(&self.config.provider_url, "/internal/snapshot")
            .await?;
        if snapshot.schema_version != 5 {
            return Err(Error::Dependency);
        }
        Ok(snapshot)
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
/// The answer of a private service.
pub(crate) struct InternalResponse {
    pub(crate) status: StatusCode,
    body: Vec<u8>,
}
impl InternalResponse {
    /// The JSON body of a successful answer.
    pub(crate) fn decode<T: DeserializeOwned>(self) -> Result<T, Error> {
        if self.status == StatusCode::NOT_FOUND {
            return Err(Error::NotFound);
        }
        if !self.status.is_success() {
            tracing::warn!(status = %self.status, "internal service returned an unsuccessful response");
            return Err(Error::Dependency);
        }
        serde_json::from_slice(&self.body).map_err(|_| Error::Dependency)
    }
}
// ponytail: bounded whole snapshot for this POC; paginate provider reads as data grows.
const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
/// Bound responses from private services before decoding their payloads.
async fn bounded_response(mut response: reqwest::Response) -> Result<Vec<u8>, Error> {
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
    Ok(bytes)
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
        protocol::AccountId32::from_bytes(random()?).to_string(),
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
