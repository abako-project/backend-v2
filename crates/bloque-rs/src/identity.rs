use std::sync::Arc;

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::config::{Auth, Config};
use crate::country::CountryCode;
use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::session::Session;

/// Profile fields for enrolling a person. Every field is optional at the
/// wire level (mirrors Bloque's own `UserProfile`), but in practice
/// `first_name`, `last_name`, and enough KYC context to identify the person
/// are needed before compliance will approve them — set what you have.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndividualProfile {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// `YYYY-MM-DD`.
    pub birthdate: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub gender: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub neighborhood: Option<String>,
    pub country_of_birth_code: Option<CountryCode>,
    pub country_of_residence_code: Option<CountryCode>,
    pub personal_id_type: Option<String>,
    pub personal_id_number: Option<String>,
}

/// Profile fields for enrolling a business (KYB). The fields below are the
/// ones Bloque requires; everything else on the underlying API is optional
/// — set it directly on the struct if you need it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BusinessProfile {
    pub legal_name: String,
    pub name: String,
    pub tax_id: String,
    #[serde(rename = "type")]
    pub business_type: String,
    /// `YYYY-MM-DD`.
    pub incorporation_date: String,
    pub address_line1: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    /// Country of incorporation, as a full name (per Bloque's own API) —
    /// not a country code. Use `country_code` for the ISO code.
    pub country: String,
    pub address_line2: Option<String>,
    pub country_code: Option<CountryCode>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub logo: Option<String>,
    pub owner_name: Option<String>,
    pub owner_id_type: Option<String>,
    pub owner_id_number: Option<String>,
    pub owner_address_line1: Option<String>,
    pub owner_address_line2: Option<String>,
    pub owner_city: Option<String>,
    pub owner_state: Option<String>,
    pub owner_postal_code: Option<String>,
    pub owner_country_code: Option<CountryCode>,
}

impl BusinessProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        legal_name: impl Into<String>,
        name: impl Into<String>,
        tax_id: impl Into<String>,
        business_type: impl Into<String>,
        incorporation_date: impl Into<String>,
        address_line1: impl Into<String>,
        city: impl Into<String>,
        state: impl Into<String>,
        postal_code: impl Into<String>,
        country: impl Into<String>,
    ) -> Self {
        Self {
            legal_name: legal_name.into(),
            name: name.into(),
            tax_id: tax_id.into(),
            business_type: business_type.into(),
            incorporation_date: incorporation_date.into(),
            address_line1: address_line1.into(),
            city: city.into(),
            state: state.into(),
            postal_code: postal_code.into(),
            country: country.into(),
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
struct EnvelopeResult<T> {
    result: T,
}

#[derive(Deserialize)]
struct AccessTokenResult {
    access_token: String,
}

/// The org-level Bloque client: authenticates as your organization (via its
/// origin key or API key) and enrolls/connects the users that belong to it.
///
/// Each successful [`register_individual`](Self::register_individual),
/// [`register_business`](Self::register_business), or [`connect`](Self::connect)
/// call hands back a [`Session`] scoped to that one user, holding its own
/// access token — this client itself never acts "as" a user.
pub struct Bloque {
    http: Arc<HttpClient>,
}

#[derive(Deserialize)]
struct Me {
    urn: String,
    origin: Option<String>,
}

impl Bloque {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            http: Arc::new(HttpClient::new(config)?),
        })
    }

    /// Shorthand for the crate's primary path: enrolling/connecting users
    /// under an origin key, against the sandbox environment.
    ///
    /// ```no_run
    /// # use bloque::Bloque;
    /// let bloque = Bloque::sandbox("my-origin", "my-origin-key")?;
    /// # Ok::<(), bloque::Error>(())
    /// ```
    pub fn sandbox(origin: impl Into<String>, origin_key: impl Into<String>) -> Result<Self> {
        Self::new(Config::sandbox(Auth::origin_key(origin_key)).with_origin(origin))
    }

    /// Shorthand for the crate's primary path: enrolling/connecting users
    /// under an origin key, against the production environment.
    pub fn production(origin: impl Into<String>, origin_key: impl Into<String>) -> Result<Self> {
        Self::new(Config::production(Auth::origin_key(origin_key)).with_origin(origin))
    }

    fn build_urn(&self, alias: &str) -> Result<String> {
        let origin = self.http.require_origin()?;
        Ok(format!("did:bloque:{origin}:{alias}"))
    }

    fn require_origin_key(&self, op: &str) -> Result<String> {
        self.http.origin_key().ok_or_else(|| {
            Error::Config(format!(
                "{op} is only available for OriginKey authentication"
            ))
        })
    }

    async fn register(
        &self,
        alias: &str,
        entity_type: &str,
        profile: serde_json::Value,
    ) -> Result<Session> {
        let origin_key = self.require_origin_key("register")?;
        let origin = self.http.require_origin()?;
        let urn = self.build_urn(alias)?;

        let body = serde_json::json!({
            "assertion_result": {
                "alias": alias,
                "challengeType": "API_KEY",
                "value": { "api_key": origin_key, "alias": alias },
            },
            "extra_context": {},
            "type": entity_type,
            "profile": profile,
        });

        let resp: EnvelopeResult<AccessTokenResult> = self
            .http
            .request(
                Method::POST,
                &format!("/api/origins/{origin}/register"),
                Some(&body),
                None,
                false,
            )
            .await?;

        let forked = self.http.fork();
        forked.set_access_token(resp.result.access_token);
        forked.set_urn(urn);
        Ok(Session::new(forked))
    }

    /// Enroll a new individual under your origin. Requires `OriginKey` auth.
    ///
    /// This is a **money-adjacent, org-authoritative action**: it creates a
    /// real identity Bloque will run KYC against. Make sure the caller (e.g.
    /// an authenticated request from your own backend) is who they claim to
    /// be before calling this.
    pub async fn register_individual(
        &self,
        alias: &str,
        profile: IndividualProfile,
    ) -> Result<Session> {
        let profile = serde_json::to_value(profile)
            .map_err(|e| Error::Config(format!("failed to serialize profile: {e}")))?;
        self.register(alias, "individual", profile).await
    }

    /// Enroll a new business (KYB) under your origin. Requires `OriginKey` auth.
    pub async fn register_business(
        &self,
        alias: &str,
        profile: BusinessProfile,
    ) -> Result<Session> {
        let profile = serde_json::to_value(profile)
            .map_err(|e| Error::Config(format!("failed to serialize profile: {e}")))?;
        self.register(alias, "business", profile).await
    }

    /// Connect to a previously-enrolled user by alias. Requires `OriginKey` auth.
    ///
    /// **This call always succeeds, even for an alias that was never
    /// registered** — the API doesn't validate identity existence here.
    /// You'll only find out later, when an account/accounts call on the
    /// returned [`Session`] fails. Track which aliases you've registered in
    /// your own application state; don't rely on `connect` to tell you.
    pub async fn connect(&self, alias: &str) -> Result<Session> {
        let origin_key = self.require_origin_key("connect(alias)")?;
        let origin = self.http.require_origin()?;
        let urn = self.build_urn(alias)?;

        let body = serde_json::json!({
            "assertion_result": {
                "challengeType": "API_KEY",
                "value": { "api_key": origin_key, "alias": alias },
            },
            "extra_context": {},
        });

        let resp: EnvelopeResult<AccessTokenResult> = self
            .http
            .request(
                Method::POST,
                &format!("/api/origins/{origin}/connect"),
                Some(&body),
                None,
                false,
            )
            .await?;

        let forked = self.http.fork();
        forked.set_access_token(resp.result.access_token);
        forked.set_urn(urn);
        Ok(Session::new(forked))
    }

    /// Resolve the identity behind the configured `ApiKey`, connecting as
    /// that single identity. There is no alias for `ApiKey` auth — the key
    /// itself determines who you are; use `OriginKey` auth to enroll or
    /// address multiple distinct users.
    pub async fn connect_api_key(&self) -> Result<Session> {
        if self.http.origin_key().is_some() {
            return Err(Error::Config(
                "connect_api_key() is only available for ApiKey authentication".into(),
            ));
        }

        let forked = self.http.fork();
        let me: Me = forked
            .request(Method::GET, "/api/identities/me", None::<&()>, None, false)
            .await?;
        forked.set_urn(me.urn);
        if let Some(origin) = me.origin {
            forked.set_origin(origin);
        }
        Ok(Session::new(forked))
    }
}
