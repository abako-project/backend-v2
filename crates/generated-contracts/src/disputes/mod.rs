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
    /// Stored public reference; reading never dereferences it.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

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

/// Fixed-width reference to an immutable adapter-owned evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCommitmentV1 {
    /// Record ID within its generation, purpose and business target.
    pub record_id: EntityId,
    /// SHA-256 of the versioned, context-bound record encoding.
    pub hash: PayloadHash,
}

/// Operation whose descriptive record is committed; SCALE order is versioned.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Encode, Decode,
)]
#[allow(clippy::cast_possible_truncation)]
pub enum EvidencePurposeV1 {
    /// A URL provided with a delivery operation, excluding later presentation text.
    #[codec(index = 0)]
    Delivery,
    /// A reason for rejecting the current submission.
    #[codec(index = 1)]
    Rejection,
    /// The initial argument opening a dispute.
    #[codec(index = 2)]
    Opening,
    /// The retained legacy single-response operation.
    #[codec(index = 3)]
    Response,
}

/// Private provider storage/read representation. The adapter resolves commitments
/// into the existing frontend reference/comment shapes before serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EvidenceRecordReferenceV1 {
    /// Historical URL reference; its original signed input remains compatible.
    Legacy(EvidenceReference),
    /// A fixed-size commitment to an adapter-owned immutable record.
    Committed(EvidenceCommitmentV1),
}
// Keep historical read/storage SCALE bytes too. A valid URL is <=2048 bytes,
// so its compact length cannot start with the big-integer marker 0xff.
impl Encode for EvidenceRecordReferenceV1 {
    fn size_hint(&self) -> usize {
        match self {
            Self::Legacy(reference) => reference.size_hint(),
            Self::Committed(_) => 50,
        }
    }
    fn encode_to<T: parity_scale_codec::Output + ?Sized>(&self, output: &mut T) {
        match self {
            Self::Legacy(reference) => reference.encode_to(output),
            Self::Committed(reference) => {
                output.write(&[0xff, 1]);
                reference.encode_to(output);
            }
        }
    }
}
impl Decode for EvidenceRecordReferenceV1 {
    fn decode<I: parity_scale_codec::Input>(
        input: &mut I,
    ) -> Result<Self, parity_scale_codec::Error> {
        let first = input.read_byte()?;
        if first == 0xff {
            if input.read_byte()? != 1 {
                return Err("unsupported evidence commitment".into());
            }
            return Ok(Self::Committed(EvidenceCommitmentV1::decode(input)?));
        }
        let length = match first & 3 {
            0 => usize::from(first >> 2),
            1 => usize::from(u16::from_le_bytes([first, input.read_byte()?]) >> 2),
            _ => return Err("invalid evidence reference length".into()),
        };
        if length > 2048 || (first & 3 == 1 && length < 64) {
            return Err("invalid evidence reference length".into());
        }
        input.on_before_alloc_mem(length)?;
        let mut bytes = vec![0; length];
        input.read(&mut bytes)?;
        let url = String::from_utf8(bytes)
            .map_err(|_| parity_scale_codec::Error::from("invalid evidence reference"))?;
        let legacy_sha256 = PayloadHash::decode(input)?;
        let mut reference = EvidenceReference::new(url)
            .map_err(|_| parity_scale_codec::Error::from("invalid evidence reference"))?;
        reference.legacy_sha256 = legacy_sha256;
        Ok(Self::Legacy(reference))
    }
}
impl From<EvidenceReference> for EvidenceRecordReferenceV1 {
    fn from(value: EvidenceReference) -> Self {
        Self::Legacy(value)
    }
}
impl EvidenceRecordReferenceV1 {
    /// An unresolved commitment has no URL; callers must resolve it explicitly.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::Legacy(reference) => Some(reference.url()),
            Self::Committed(_) => None,
        }
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
        reason: EvidenceRecordReferenceV1,
        /// Verified client account.
        reviewed_by: AccountId32,
        /// Provider timestamp.
        reviewed_at: UnixSeconds,
    },
    /// Refused with a private adapter comment bound to this delivery.
    #[codec(index = 3)]
    RejectedWithComment {
        /// Immutable adapter reason identity.
        comment_id: EntityId,
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
    /// Optional submitted artifact; descriptive links live in the adapter.
    pub deliverable: Option<EvidenceRecordReferenceV1>,
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

/// Opening references an immutable argument verified and stored by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "Explicit resource IDs identify the immutable opening target."
)]
pub struct OpenDisputeWithCommentRequest {
    /// Target project.
    pub project_id: EntityId,
    /// Target milestone.
    pub milestone_id: EntityId,
    /// Exact current rejected submission.
    pub rejected_submission_id: EntityId,
    /// Adapter-owned opening argument.
    pub comment_id: EntityId,
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
    pub evidence: EvidenceRecordReferenceV1,
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
    /// Earlier reference-based opening, absent for a written argument.
    pub evidence: Option<EvidenceRecordReferenceV1>,
    /// Immutable adapter opening argument, confirmed by this case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening_comment_id: Option<EntityId>,
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
