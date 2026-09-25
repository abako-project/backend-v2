use std::sync::Arc;

use crate::accounts::AccountsClient;
use crate::compliance::KycClient;
use crate::config::{Auth, Config, Mode};
use crate::error::Result;
use crate::http::HttpClient;
use crate::swap::SwapClient;

/// A connection scoped to a single enrolled user — returned by
/// [`crate::Bloque::register_individual`], [`crate::Bloque::register_business`],
/// [`crate::Bloque::connect`], [`crate::Bloque::connect_api_key`], or
/// [`Session::from_token`].
///
/// Holds that user's own access token (via an internally forked HTTP
/// client), so operations performed through it are scoped to them, not to
/// your organization's own identity.
pub struct Session {
    http: Arc<HttpClient>,
}

impl Session {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// Wrap an access token you already have into a `Session`, skipping
    /// this crate's own register/connect handshake entirely. For a token
    /// from a JWT/OTP login done via the official TS SDK, a saved browser
    /// or CLI session, or anywhere else outside this crate — `urn` and
    /// `origin` should be that same identity's own values (e.g. an
    /// `identities/me` response, or a saved session file), since there's no
    /// server round-trip here to derive them.
    ///
    /// ```no_run
    /// # use bloque::{Mode, Session};
    /// let session = Session::from_token(
    ///     Mode::Sandbox,
    ///     "bloque-email",
    ///     "did:bloque:bloque-email:me@example.com",
    ///     "eyJhbGciOi...",
    /// )?;
    /// # Ok::<(), bloque::Error>(())
    /// ```
    pub fn from_token(
        mode: Mode,
        origin: impl Into<String>,
        urn: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Result<Self> {
        let config = Config::new(mode, Auth::token(access_token)).with_origin(origin);
        let http = Arc::new(HttpClient::new(config)?);
        http.set_urn(urn);
        Ok(Self::new(http))
    }

    /// The user's URN, e.g. `did:bloque:my-origin:@alice`.
    pub fn urn(&self) -> String {
        self.http.urn().unwrap_or_default()
    }

    /// The user's short-lived JWT. Not meant for long-term storage — reissue
    /// a session via `connect()` instead of caching this across requests.
    pub fn access_token(&self) -> Option<String> {
        self.http.access_token()
    }

    pub fn accounts(&self) -> AccountsClient {
        AccountsClient::new(self.http.clone())
    }

    pub fn swap(&self) -> SwapClient {
        SwapClient::new(self.http.clone())
    }

    /// KYC/KYB verification. Pass this session's own [`Session::urn`] to
    /// verify the user themselves, or an organization URN to verify that
    /// org instead.
    pub fn kyc(&self) -> KycClient {
        KycClient::new(self.http.clone())
    }
}
