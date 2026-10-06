use std::time::Duration;

/// Which Bloque environment to hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Sandbox,
    Production,
}

impl Mode {
    pub(crate) fn base_url(self) -> &'static str {
        match self {
            Mode::Sandbox => "https://api.dev-bloque.app",
            Mode::Production => "https://api.bloque.app",
        }
    }
}

/// Authentication strategy. This crate only targets backend/server
/// integrations (an organization enrolling and managing its own users) —
/// there's no browser/JWT support here, unlike the TS SDK.
#[derive(Debug, Clone)]
pub enum Auth {
    /// `sk_live_...` / `sk_test_...` secret key. Exchanged automatically for
    /// a short-lived JWT on first use. `connect()` resolves to the single
    /// identity the key itself represents (via `/api/identities/me`) — it
    /// cannot enroll or address other users.
    ApiKey {
        api_key: String,
        scopes: Vec<String>,
    },
    /// Legacy origin-scoped key. Required for [`crate::Bloque::register_individual`],
    /// [`crate::Bloque::register_business`], and [`crate::Bloque::connect`] — the
    /// enrollment flows this crate is built around.
    OriginKey { origin_key: String },
    /// An access token obtained some other way — a JWT/OTP login done via
    /// the official TS SDK, a saved CLI session, ... This crate doesn't
    /// implement that login flow itself; this variant just lets you use the
    /// resulting token. Every request sends `Authorization: Bearer
    /// <access_token>` as-is — no exchange, no origin-key header. Pair with
    /// [`crate::Session::from_token`], not [`crate::Bloque`] — the token
    /// already identifies one specific user, so there's no separate
    /// register/connect step.
    Token { access_token: String },
}

impl Auth {
    /// Shorthand for `Auth::ApiKey { api_key: ..., scopes: vec![] }`.
    pub fn api_key(api_key: impl Into<String>) -> Self {
        Self::ApiKey {
            api_key: api_key.into(),
            scopes: Vec::new(),
        }
    }

    /// Shorthand for `Auth::ApiKey` with scope narrowing.
    pub fn api_key_scoped(api_key: impl Into<String>, scopes: Vec<String>) -> Self {
        Self::ApiKey {
            api_key: api_key.into(),
            scopes,
        }
    }

    /// Shorthand for `Auth::OriginKey { origin_key: ... }`.
    pub fn origin_key(origin_key: impl Into<String>) -> Self {
        Self::OriginKey {
            origin_key: origin_key.into(),
        }
    }

    /// Shorthand for `Auth::Token { access_token: ... }`.
    pub fn token(access_token: impl Into<String>) -> Self {
        Self::Token {
            access_token: access_token.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub enabled: bool,
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryConfig {
    /// No retries — fail on the first error. Useful in tests, or when your
    /// own caller already retries.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// The origin namespace (e.g. your Bloque-assigned org identifier).
    /// Required for `OriginKey` auth; optional for `ApiKey` auth.
    pub origin: Option<String>,
    pub auth: Auth,
    pub timeout: Duration,
    pub retry: RetryConfig,
    /// Override the environment's default base URL. Mainly for testing
    /// against a local mock server.
    pub base_url: Option<String>,
}

impl Config {
    pub fn new(mode: Mode, auth: Auth) -> Self {
        Self {
            mode,
            origin: None,
            auth,
            timeout: Duration::from_secs(30),
            retry: RetryConfig::default(),
            base_url: None,
        }
    }

    /// Shorthand for `Config::new(Mode::Sandbox, auth)`.
    pub fn sandbox(auth: Auth) -> Self {
        Self::new(Mode::Sandbox, auth)
    }

    /// Shorthand for `Config::new(Mode::Production, auth)`.
    pub fn production(auth: Auth) -> Self {
        Self::new(Mode::Production, auth)
    }

    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    pub(crate) fn validate(&self) -> crate::Result<()> {
        if let Auth::OriginKey { origin_key } = &self.auth {
            if origin_key.trim().is_empty() {
                return Err(crate::Error::Config(
                    "origin key is required for OriginKey authentication".into(),
                ));
            }
            if self.origin.as_deref().unwrap_or("").trim().is_empty() {
                return Err(crate::Error::Config(
                    "origin is required for OriginKey authentication".into(),
                ));
            }
        }
        if let Auth::ApiKey { api_key, .. } = &self.auth
            && api_key.trim().is_empty()
        {
            return Err(crate::Error::Config(
                "api key is required for ApiKey authentication".into(),
            ));
        }
        if let Auth::Token { access_token } = &self.auth
            && access_token.trim().is_empty()
        {
            return Err(crate::Error::Config(
                "access_token is required for Token authentication".into(),
            ));
        }
        Ok(())
    }
}
