//! Public references, immutable delivery records and dispute contracts.

use crate::{
    AccountId32, ContractError, EntityId, MilestoneView, PayloadHash, UnixSeconds, WorkerRating,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// A public HTTPS reference. Recording it does not fetch or verify a document.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Encode)]
#[serde(try_from = "EvidenceWire", into = "EvidenceWire")]
pub struct EvidenceReference {
    url: String,
    // Retain the SCALE slot for already signed operations; new evidence has no digest.
    legacy_sha256: PayloadHash,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceWire {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

impl EvidenceReference {
    /// Validate an absolute HTTPS reference without contacting its host.
    pub fn new(url: String) -> Result<Self, ContractError> {
        if url.len() > 2048
            || url.chars().any(char::is_whitespace)
            || url.chars().any(char::is_control)
            || !url.starts_with("https://")
        {
            return Err(ContractError::Invalid("evidence"));
        }
        let parsed = url::Url::parse(&url).map_err(|_| ContractError::Invalid("evidence"))?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ContractError::Invalid("evidence"));
        }
        Ok(Self {
            url,
            legacy_sha256: PayloadHash::from_bytes([0; 32]),
        })
    }
}

impl std::fmt::Debug for EvidenceReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EvidenceReference { [REDACTED] }")
    }
}
impl TryFrom<EvidenceWire> for EvidenceReference {
    type Error = ContractError;
    fn try_from(value: EvidenceWire) -> Result<Self, Self::Error> {
        let mut reference = Self::new(value.url)?;
        reference.legacy_sha256 = value
            .sha256
            .and_then(|hash| hash.parse().ok())
            .unwrap_or(PayloadHash::from_bytes([0; 32]));
        Ok(reference)
    }
}
impl From<EvidenceReference> for EvidenceWire {
    fn from(value: EvidenceReference) -> Self {
        Self {
            url: value.url,
            // Preserve old signed JSON commands exactly; new records omit this obsolete field.
            sha256: (value.legacy_sha256 != PayloadHash::from_bytes([0; 32]))
                .then(|| value.legacy_sha256.to_string()),
        }
    }
}
impl Decode for EvidenceReference {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let url = String::decode(input)?;
        let legacy_sha256 = PayloadHash::decode(input)?;
        let mut reference = Self::new(url)
            .map_err(|_| parity_scale_codec::Error::from("invalid evidence reference"))?;
        reference.legacy_sha256 = legacy_sha256;
        Ok(reference)
    }
}

/// One terminal review, bound to the exact submission rather than a mutable URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", rename_all_fields = "camelCase", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation)]
pub enum SubmissionReview {
    /// Awaiting the client's decision.
    #[codec(index = 0)]
    PendingReview,
    /// Refused by the client with a public reason reference.
    #[codec(index = 1)]
    Rejected {
        /// Reason document.
        reason: EvidenceReference,
        /// Verified client account.
        reviewed_by: AccountId32,
        /// Provider timestamp.
        reviewed_at: UnixSeconds,
    },
    /// Accepted and settled atomically.
    #[codec(index = 2)]
    Accepted {
        /// Verified client account.
        reviewed_by: AccountId32,
        /// Provider timestamp.
        reviewed_at: UnixSeconds,
    },
}

/// A milestone delivery whose content and ratings cannot be overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionSubmission {
    /// Provider-generated identity.
    pub submission_id: EntityId,
    /// One-based, increasing within the milestone.
    pub version: u64,
    /// Submitted artifact reference.
    pub deliverable: EvidenceReference,
    /// Verified coordinator.
    pub submitted_by: AccountId32,
    /// Provider timestamp.
    pub submitted_at: UnixSeconds,
    /// Existing coordinator ratings used only on acceptance.
    pub worker_ratings: Vec<WorkerRating>,
    /// Current review result.
    pub review: SubmissionReview,
}

/// Public evidence body for rejection, opening or response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRequest {
    /// Public evidence reference.
    pub evidence: EvidenceReference,
}

/// The opening explicitly names the currently rejected delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenDisputeRequest {
    /// Target project.
    pub project_id: EntityId,
    /// Target milestone.
    pub milestone_id: EntityId,
    /// Current rejected delivery.
    pub rejected_submission_id: EntityId,
    /// Opener's argument reference.
    pub evidence: EvidenceReference,
}

/// This proof of concept has no transition to a resolved state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum DisputeStatus {
    /// Open, publicly readable, with its project frozen.
    #[codec(index = 0)]
    Open,
}

/// The counterparty's single append-only intervention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisputeResponse {
    /// Public response reference.
    pub evidence: EvidenceReference,
    /// Verified counterparty.
    pub author: AccountId32,
    /// Provider timestamp.
    pub occurred_at: UnixSeconds,
}

/// Provider-owned case. It references frozen records, not copied snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dispute {
    /// Case identity.
    #[allow(
        clippy::struct_field_names,
        reason = "The public contract uses disputeId consistently."
    )]
    pub dispute_id: EntityId,
    /// Frozen project identity.
    pub project_id: EntityId,
    /// Disputed milestone identity.
    pub milestone_id: EntityId,
    /// Exact rejected submission.
    pub rejected_submission_id: EntityId,
    /// Open throughout this proof of concept.
    pub status: DisputeStatus,
    /// Verified opener.
    pub opened_by: AccountId32,
    /// Other project principal.
    pub counterparty: AccountId32,
    /// Provider opening timestamp.
    pub opened_at: UnixSeconds,
    /// Public opening reference.
    pub evidence: EvidenceReference,
    /// Present after exactly one counterparty response.
    pub response: Option<DisputeResponse>,
    /// Proposal revision at opening; not a historical task revision.
    pub proposal_revision: u64,
    /// Last committed event before opening.
    pub context_event_cursor: u64,
}

/// Explicit anonymous read projection, excluding internal transport/notification data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisputeView {
    /// Public case record.
    pub dispute: Dispute,
    /// Only the affected frozen milestone, including its submission history.
    pub milestone: MilestoneView,
}

#[cfg(test)]
mod tests;
