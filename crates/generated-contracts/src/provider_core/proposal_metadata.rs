//! Explicit proposal-context preimages; no presentation sections or task content.
use super::{MetadataCommitmentV1, ProposalContentReferenceV1};
use crate::{
    AccountId32, EntityId, OperationId, PayloadHash, ProposalDefinition, ProviderCommand,
    ProviderInstanceId,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Header for owner-controlled proposal NDJSON exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalMetadataHeaderV1 {
    /// Exchange version, currently 1.
    pub schema_version: u16,
    /// Exact retained generation.
    pub generation: ProviderInstanceId,
}
/// Actual signed operation or explicit migration; never an invented old approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum ProposalProvenanceV1 {
    /// Target is unknown for Create, explicit for Update/change request.
    #[codec(index = 0)]
    Operation {
        /// Existing signing operation.
        operation_id: OperationId,
        /// Verified origin.
        author: AccountId32,
        /// Existing target for non-creation operations.
        proposal_id: Option<EntityId>,
    },
    /// Current retained value, without claiming a historical signer.
    #[codec(index = 1)]
    Migration {
        /// Hash of exact immutable source-file bytes.
        source_archive_hash: PayloadHash,
        /// Retained proposal identity.
        proposal_id: EntityId,
    },
}
/// Two independent immutable values belonging to the same proposal context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum ProposalContentV1 {
    /// Original quoted inputs, including exact human text and numeric terms.
    #[codec(index = 0)]
    Definition(ProposalDefinition),
    /// Exact client explanation, separate from the current definition.
    #[codec(index = 1)]
    ChangeRequest(String),
}
/// Immutable API-owned proposal record, bound to context and operation/source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalMetadataRecordV1 {
    /// Provider generation, fixed before signing.
    pub generation: ProviderInstanceId,
    /// Parent Project; never inferred from a mutable projection.
    pub project_id: EntityId,
    /// Operation or explicit retained identity.
    pub provenance: ProposalProvenanceV1,
    /// Original definition or change-request text.
    pub content: ProposalContentV1,
}
impl ProposalMetadataRecordV1 {
    /// Build from original accepted request, preserving raw budget inputs.
    #[must_use]
    pub fn from_command(
        generation: ProviderInstanceId,
        author: AccountId32,
        operation_id: OperationId,
        command: &ProviderCommand,
    ) -> Option<Self> {
        let (project_id, proposal_id, content) = match command {
            ProviderCommand::CreateProposal {
                project_id,
                proposal,
            } => (
                *project_id,
                None,
                ProposalContentV1::Definition(proposal.clone()),
            ),
            ProviderCommand::UpdateProposal {
                project_id,
                proposal_id,
                proposal,
            } => (
                *project_id,
                Some(*proposal_id),
                ProposalContentV1::Definition(proposal.clone()),
            ),
            ProviderCommand::RequestProposalChanges {
                project_id,
                proposal_id,
                request,
            } => (
                *project_id,
                Some(*proposal_id),
                ProposalContentV1::ChangeRequest(request.reference.clone()),
            ),
            _ => return None,
        };
        Some(Self {
            generation,
            project_id,
            provenance: ProposalProvenanceV1::Operation {
                operation_id,
                author,
                proposal_id,
            },
            content,
        })
    }
    /// Check selected owner/entity before using any text. Verify digest separately.
    #[must_use]
    pub fn matches_context(
        &self,
        generation: ProviderInstanceId,
        project_id: EntityId,
        proposal_id: EntityId,
        author: AccountId32,
        reference: &ProposalContentReferenceV1,
    ) -> bool {
        if self.generation != generation || self.project_id != project_id {
            return false;
        }
        match (&self.provenance, reference) {
            (
                ProposalProvenanceV1::Operation {
                    operation_id,
                    author: origin,
                    proposal_id: target,
                },
                ProposalContentReferenceV1::Signed(MetadataCommitmentV1 {
                    operation_id: selected,
                    ..
                }),
            ) => {
                *operation_id == *selected
                    && *origin == author
                    && target.map_or(
                        matches!(self.content, ProposalContentV1::Definition(_)),
                        |target| target == proposal_id,
                    )
            }
            (
                ProposalProvenanceV1::Migration {
                    proposal_id: target,
                    ..
                },
                ProposalContentReferenceV1::Migration(_),
            ) => *target == proposal_id,
            _ => false,
        }
    }
    /// SHA-256 preimage; encode/hash on a blocking worker at async boundaries.
    #[must_use]
    pub fn hash_input(&self) -> Vec<u8> {
        let mut bytes = b"abako/proposal-document/v1\0".to_vec();
        self.encode_to(&mut bytes);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::DecodeAll;
    use std::fmt::Write;
    #[test]
    fn proposal_record_codec_matches_independent_operation_and_migration_vectors()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../specs/0015-adapter-core-boundary/proposal-metadata-vectors.json"
        ))?;
        for vector in fixture["vectors"].as_array().ok_or("vectors")? {
            let record: ProposalMetadataRecordV1 =
                serde_json::from_value(vector["record"].clone())?;
            let mut hex = String::new();
            for byte in record.hash_input() {
                write!(&mut hex, "{byte:02x}")?;
            }
            assert_eq!(hex, vector["hashInputHex"]);
            assert_eq!(
                ProposalMetadataRecordV1::decode_all(&mut &record.encode()[..])?,
                record
            );
        }
        Ok(())
    }
}
