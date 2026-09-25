use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;

use crate::error::Result;
use crate::http::HttpClient;
use crate::util::wire_string_enum;

wire_string_enum! {
    /// Whether a verification is KYC (individual) or KYB (organization).
    /// Bloque derives this from the URN's own shape — you don't choose it.
    pub enum ComplianceType {
        Kyc => "kyc",
        Kyb => "kyb",
    }
}

wire_string_enum! {
    /// Compliance depth requested. Only `basic` exists today.
    pub enum ComplianceLevel {
        Basic => "basic",
    }
}

wire_string_enum! {
    /// Third-party provider actually running the verification.
    pub enum ComplianceProvider {
        Amlbot => "AMLBOT",
        Sumsub => "SUMSUB",
    }
}

wire_string_enum! {
    /// Current state of a KYC/KYB verification.
    pub enum VerificationStatus {
        AwaitingComplianceVerification => "awaiting_compliance_verification",
        Approved => "approved",
        Rejected => "rejected",
    }
}

/// A KYC/KYB verification's current state, from
/// [`KycClient::start_verification`] or [`KycClient::get_verification`].
#[derive(Debug, Clone, Deserialize)]
pub struct KycVerification {
    #[serde(rename = "type")]
    pub compliance_type: ComplianceType,
    pub level: ComplianceLevel,
    pub provider: ComplianceProvider,
    pub status: VerificationStatus,
    /// URL to send the subject to, to complete or view the verification.
    /// This is a hosted flow — nothing your backend does can substitute for
    /// the subject opening this link themselves.
    pub url: String,
    /// `None` until `status` leaves `awaiting_compliance_verification`.
    pub completed_at: Option<String>,
    /// Present once document retrieval has run, when enabled server-side.
    pub documents_status: Option<String>,
    /// Raw provider-specific verification payload, when available.
    pub result: Option<serde_json::Value>,
}

// `start` and `get` return differently-shaped bodies (`get` adds
// completion/document/result fields, and even renames `url` to
// `verification_url`) — deserialize each into its own raw shape, then map
// both into the one `KycVerification` callers actually work with.

#[derive(Deserialize)]
struct StartRaw {
    #[serde(rename = "type")]
    compliance_type: ComplianceType,
    level: ComplianceLevel,
    provider: ComplianceProvider,
    status: VerificationStatus,
    url: String,
}

impl From<StartRaw> for KycVerification {
    fn from(r: StartRaw) -> Self {
        Self {
            compliance_type: r.compliance_type,
            level: r.level,
            provider: r.provider,
            status: r.status,
            url: r.url,
            completed_at: None,
            documents_status: None,
            result: None,
        }
    }
}

#[derive(Deserialize)]
struct GetRaw {
    #[serde(rename = "type")]
    compliance_type: ComplianceType,
    level: ComplianceLevel,
    provider: ComplianceProvider,
    status: VerificationStatus,
    verification_url: String,
    completed_at: Option<String>,
    #[serde(default)]
    documents_status: Option<String>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

impl From<GetRaw> for KycVerification {
    fn from(r: GetRaw) -> Self {
        Self {
            compliance_type: r.compliance_type,
            level: r.level,
            provider: r.provider,
            status: r.status,
            url: r.verification_url,
            completed_at: r.completed_at,
            documents_status: r.documents_status,
            result: r.result,
        }
    }
}

/// One stored KYC document image, from [`KycClient::get_documents`].
#[derive(Debug, Clone, Deserialize)]
pub struct KycDocument {
    pub document_type: String,
    pub side: String,
    /// `None` if storage failed — never a hard error on its own.
    pub image_s3_key: Option<String>,
    pub image_size_bytes: u64,
    /// Short-lived presigned download URL. `None` if presigning failed
    /// (e.g. no storage client configured server-side).
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KycDocuments {
    pub documents_status: String,
    pub documents: Vec<KycDocument>,
}

/// KYC/KYB verification for one [`crate::Session`]'s user (or, given an
/// organization URN, for that org). Mirrors `user.compliance.kyc.*` in the
/// official SDK — this crate only implements the KYC piece of Bloque's
/// wider compliance module (not tiers, TOS gate, or the verification gate).
pub struct KycClient {
    http: Arc<HttpClient>,
}

impl KycClient {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// Start verification for `urn` — an identity URN starts KYC, an
    /// organization URN (`did:bloque:orgs:{id}`) starts KYB. Returns a URL
    /// the subject must open themselves to complete it.
    pub async fn start_verification(&self, urn: &str) -> Result<KycVerification> {
        let raw: StartRaw = self
            .http
            .request(
                Method::POST,
                "/api/compliance",
                Some(&serde_json::json!({ "urn": urn })),
                None,
                false,
            )
            .await?;
        Ok(raw.into())
    }

    /// Check current verification status for `urn`.
    pub async fn get_verification(&self, urn: &str) -> Result<KycVerification> {
        let raw: GetRaw = self
            .http
            .request(
                Method::GET,
                &format!("/api/compliance/{urn}"),
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(raw.into())
    }

    /// Fetch stored document images for `urn`'s verification, when document
    /// retrieval is enabled server-side.
    pub async fn get_documents(&self, urn: &str) -> Result<KycDocuments> {
        self.http
            .request(
                Method::GET,
                &format!("/api/compliance/{urn}/documents"),
                None::<&()>,
                None,
                false,
            )
            .await
    }
}
