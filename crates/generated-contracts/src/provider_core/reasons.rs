//! Lifecycle reason context; financial/freeze facts remain independently core-owned.
use super::MetadataCommitmentV1;
use crate::{
    AccountId32, DomainEventKind, EntityId, OperationId, PayloadHash, ProviderCommand,
    ProviderInstanceId, UnixSeconds,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Compact reason content selection, with explicit migration provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum ReasonReferenceV1 {
    /// Actual accepted signing operation.
    #[codec(index = 0)]
    Write(MetadataCommitmentV1),
    /// Explicit retained value from an owner export.
    #[codec(index = 1)]
    Migration(PayloadHash),
}
impl ReasonReferenceV1 {
    /// Expected exact context digest.
    #[must_use]
    pub const fn hash(&self) -> PayloadHash {
        match self {
            Self::Write(v) => v.hash,
            Self::Migration(v) => *v,
        }
    }
}
/// Ordered reason facts, without the written explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReasonCore {
    /// Affected Project.
    pub project_id: EntityId,
    /// Retained milestone when historical data contains one.
    pub milestone_id: Option<EntityId>,
    /// Actual verified actor.
    pub origin: AccountId32,
    /// Selected offchain explanation.
    pub content: ReasonReferenceV1,
    /// Actual execution time, not predicted by API staging.
    pub occurred_at: UnixSeconds,
    /// Original contractual effect.
    pub kind: DomainEventKind,
}
/// Protected owner-exchange header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReasonMetadataHeaderV1 {
    /// Context version, currently1.
    pub schema_version: u16,
    /// Exact provider generation.
    pub generation: ProviderInstanceId,
}
/// Real operation or current retained value; no invented historical signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum ReasonProvenanceV1 {
    /// The timestamp is determined during actual execution, outside this input.
    #[codec(index = 0)]
    Operation {
        /// Original operation identity.
        operation_id: OperationId,
        /// Actual authorized origin.
        author: AccountId32,
    },
    /// Exact archive and retained ordered position.
    #[codec(index = 1)]
    Migration {
        /// Hash exact source bytes.
        source_archive_hash: PayloadHash,
        /// Zero-based original global reason position.
        record_index: u64,
        /// Retained actor, without claiming a signature.
        origin: AccountId32,
        /// Retained timestamp.
        occurred_at: UnixSeconds,
    },
}
/// Exact immutable written explanation, bound to its lifecycle context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReasonMetadataRecordV1 {
    /// Generation fixed before signing.
    pub generation: ProviderInstanceId,
    /// Signed operation or explicit migration provenance.
    pub provenance: ReasonProvenanceV1,
    /// Existing Project identity.
    pub project_id: EntityId,
    /// Historical milestone identity, when present.
    pub milestone_id: Option<EntityId>,
    /// Actual reason action; cancellation and planning dispute are distinct.
    pub kind: DomainEventKind,
    /// Exact Unicode/control-preserving explanation.
    pub reason: String,
}
impl ReasonMetadataRecordV1 {
    /// Stage the original public input without predicting revision/time.
    #[must_use]
    pub fn from_command(
        generation: ProviderInstanceId,
        author: AccountId32,
        operation_id: OperationId,
        command: &ProviderCommand,
    ) -> Option<Self> {
        let (project_id, request, kind) = match command {
            ProviderCommand::CancelProject {
                project_id,
                request,
            } => (*project_id, request, DomainEventKind::ProjectCancelled),
            ProviderCommand::DisputePlanning {
                project_id,
                request,
            } => (*project_id, request, DomainEventKind::PlanningDisputed),
            _ => return None,
        };
        Some(Self {
            generation,
            provenance: ReasonProvenanceV1::Operation {
                operation_id,
                author,
            },
            project_id,
            milestone_id: None,
            kind,
            reason: request.reason.clone(),
        })
    }
    /// Verify context before using text; the owner separately checks the digest.
    #[must_use]
    pub fn matches_context(
        &self,
        generation: ProviderInstanceId,
        record_index: u64,
        core: &ReasonCore,
    ) -> bool {
        if self.generation != generation
            || self.project_id != core.project_id
            || self.milestone_id != core.milestone_id
            || self.kind != core.kind
        {
            return false;
        }
        match (&self.provenance, &core.content) {
            (
                ReasonProvenanceV1::Operation {
                    operation_id,
                    author,
                },
                ReasonReferenceV1::Write(selected),
            ) => *operation_id == selected.operation_id && *author == core.origin,
            (
                ReasonProvenanceV1::Migration {
                    origin,
                    occurred_at,
                    record_index: selected_index,
                    ..
                },
                ReasonReferenceV1::Migration(_),
            ) => {
                *origin == core.origin
                    && *occurred_at == core.occurred_at
                    && *selected_index == record_index
            }
            _ => false,
        }
    }
    /// SHA256 preimage; encode/hash away from async runtime threads.
    #[must_use]
    pub fn hash_input(&self) -> Vec<u8> {
        let mut bytes = b"abako/lifecycle-reason/v1\0".to_vec();
        self.encode_to(&mut bytes);
        bytes
    }
}

/// Bounded private owner read. Written reason preimages never enter this response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReasonPageV1 {
    /// Exact current deployment generation.
    pub generation: ProviderInstanceId,
    /// Original zero-based first record position.
    pub start: u64,
    /// Exclusive next record position.
    pub next: u64,
    /// Current total; owner exchange requires a paused stable cohort.
    pub total: u64,
    /// At most256 compact records.
    pub reasons: Vec<ReasonCore>,
}
