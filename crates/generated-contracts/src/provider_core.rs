//! Private provider protocol for SPEC-0015. These are not public REST request DTOs.
use crate::{CalendarDefinition, OperationId, PayloadHash, Qualifications};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

mod metadata;
pub use metadata::*;
mod models;
pub use models::*;
mod proposal;
pub use proposal::*;

/// Fixed reference to an immutable adapter-owned creation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataCommitmentV1 {
    /// Existing operation identity; metadata does not consume another random ID.
    pub operation_id: OperationId,
    /// Context-bound SHA-256 record commitment; validation belongs to its owner.
    pub hash: PayloadHash,
}
/// Worker facts sent to the core without a display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterWorkerCoreRequest {
    /// Adapter-owned immutable directory-name record.
    pub content: MetadataCommitmentV1,
    /// Role/skill memberships still enforced by the core.
    pub qualifications: Qualifications,
    /// Capacity and override facts still enforced by the core.
    pub calendar: CalendarDefinition,
}
