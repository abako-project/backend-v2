//! Offchain preimages for the two immutable creation fields moving in R2a.
//! This is a private codec, never a contract input or public REST document.
use crate::{AccountId32, EntityId, OperationId, ProviderCommand, ProviderInstanceId};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// SHA-256 commits this domain followed by the SCALE encoding of the record.
/// Consumers use their existing SHA-256 implementation; this data-only crate
/// does not pull a signing or hashing implementation into the frontend.
pub const CREATION_METADATA_DOMAIN_V1: &[u8] = b"abako/creation-metadata/v1\0";

/// First NDJSON line in an owner-controlled metadata exchange. Following lines
/// are `CreationMetadataRecordV1` values verified against a selected core snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreationMetadataHeaderV1 {
    /// File format version; the current explicit exchange supports only 1.
    pub schema_version: u16,
    /// Exact target/provider generation, never reassigned during import.
    pub generation: ProviderInstanceId,
}

/// An entity restored from the exact source archive, not a historical signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum MetadataEntityV1 {
    /// Directory worker identified by its account.
    #[codec(index = 0)]
    Worker(AccountId32),
    /// Project identified by its retained entity ID.
    #[codec(index = 1)]
    Project(EntityId),
}

/// Distinguishes signed creations from a later, explicit migration commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum MetadataProvenanceV1 {
    /// The original operation and verified signer. No additional random ID.
    #[codec(index = 0)]
    Creation {
        /// Operation already assigned by the API or verified signed envelope.
        operation_id: OperationId,
        /// Origin verified by the signing boundary.
        author: AccountId32,
    },
    /// The importer has retained this entity from an immutable owner export.
    /// This makes no claim about its historical creation transaction or signer.
    #[codec(index = 1)]
    Migration {
        /// Hash of the exact immutable export bytes, shared by all its records.
        source_archive_hash: crate::PayloadHash,
        /// Retained identity whose descriptive content was exported.
        entity: MetadataEntityV1,
    },
}

/// Only creation content; qualifications, calendars and proposals are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum CreationContentV1 {
    /// Immutable directory name, distinct from a mutable identity/profile name.
    #[codec(index = 0)]
    WorkerName {
        /// Exact accepted UTF-8, without trimming or Unicode normalization.
        display_name: String,
    },
    /// Immutable top-level Project text, without proposal/task children.
    #[codec(index = 1)]
    ProjectText {
        /// Exact accepted Project title.
        title: String,
        /// Exact accepted Project description.
        description: String,
    },
}

/// Immutable API-owned record. Generation and provenance prevent substitution
/// across accounts, operations, imports or provider resets. V1 is identified by
/// its fixed hashing domain and enum indexes, not by a mutable version field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreationMetadataRecordV1 {
    /// Provider generation accepting the operation or retained by migration.
    pub generation: ProviderInstanceId,
    /// Signed creation or explicitly migration-time provenance.
    pub provenance: MetadataProvenanceV1,
    /// Exact descriptive preimage, retained offchain only.
    pub content: CreationContentV1,
}
impl CreationMetadataRecordV1 {
    /// Check the selected core entity's context. The owner must additionally
    /// compare SHA-256 of `hash_input()` to `reference.hash()` before using any text.
    #[must_use]
    pub fn matches_context(
        &self,
        generation: ProviderInstanceId,
        entity: &MetadataEntityV1,
        owner: AccountId32,
        reference: &super::MetadataReferenceV1,
    ) -> bool {
        if self.generation != generation
            || !matches!(
                (&self.content, entity),
                (
                    CreationContentV1::WorkerName { .. },
                    MetadataEntityV1::Worker(_)
                ) | (
                    CreationContentV1::ProjectText { .. },
                    MetadataEntityV1::Project(_)
                )
            )
        {
            return false;
        }
        match (&self.provenance, reference) {
            (
                MetadataProvenanceV1::Creation {
                    operation_id,
                    author,
                },
                super::MetadataReferenceV1::Creation(content),
            ) => *operation_id == content.operation_id && *author == owner,
            (
                MetadataProvenanceV1::Migration {
                    entity: retained, ..
                },
                super::MetadataReferenceV1::Migration(_),
            ) => retained == entity,
            _ => false,
        }
    }

    /// Construct from the original accepted request, never from a current worker
    /// view whose qualifications and calendar may have changed since creation.
    #[must_use]
    pub fn from_creation_command(
        generation: ProviderInstanceId,
        author: AccountId32,
        operation_id: OperationId,
        command: &ProviderCommand,
    ) -> Option<Self> {
        let content = match command {
            ProviderCommand::RegisterWorker(request) => CreationContentV1::WorkerName {
                display_name: request.display_name.clone(),
            },
            ProviderCommand::CreateProject(request) => CreationContentV1::ProjectText {
                title: request.title.clone(),
                description: request.description.clone(),
            },
            _ => return None,
        };
        Some(Self {
            generation,
            provenance: MetadataProvenanceV1::Creation {
                operation_id,
                author,
            },
            content,
        })
    }

    /// Exact SHA-256 preimage. Large content encoding belongs on a blocking
    /// worker at an async boundary; no contract is given these bytes.
    #[must_use]
    pub fn hash_input(&self) -> Vec<u8> {
        let mut input = CREATION_METADATA_DOMAIN_V1.to_vec();
        self.encode_to(&mut input);
        input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::DecodeAll;
    use std::fmt::Write;

    #[test]
    fn creation_metadata_codec_matches_independent_signed_and_migration_vectors()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../specs/0015-adapter-core-boundary/creation-metadata-vectors.json"
        ))?;
        for vector in fixture["vectors"].as_array().ok_or("vectors missing")? {
            let record: CreationMetadataRecordV1 =
                serde_json::from_value(vector["record"].clone())?;
            let mut hex = String::new();
            for byte in record.hash_input() {
                write!(&mut hex, "{byte:02x}")?;
            }
            assert_eq!(hex, vector["hashInputHex"]);
            assert_eq!(
                CreationMetadataRecordV1::decode_all(&mut &record.encode()[..])?,
                record
            );
        }
        Ok(())
    }
}
