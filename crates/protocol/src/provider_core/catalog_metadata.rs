//! Immutable catalog documents. This is one context, not a content registry.
use super::{CatalogReferenceV1, MetadataCommitmentV1};
use crate::{
    AccountId32, CatalogKind, CreateSkillRequest, OperationId, ProviderCommand, ProviderInstanceId,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Source of the catalog value; seed never impersonates a signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "Three fixed SCALE variants."
)]
pub enum CatalogProvenanceV1 {
    /// Real operation; request ID is allocated later by core.
    #[codec(index = 0)]
    Operation {
        /// Existing operation identity.
        operation_id: OperationId,
        /// Verified root or worker origin.
        author: AccountId32,
    },
    /// Explicit reviewed default dataset; still bound to generation and target.
    #[codec(index = 2)]
    Seed {
        /// Default dataset version, currently1.
        dataset_version: u16,
    },
}
/// Preserve exact labels/category and independently scoped request content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(
    tag = "type",
    content = "data",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "Three fixed SCALE variants."
)]
pub enum CatalogContentV1 {
    /// Current label; category matters only when selected as an actual origin.
    #[codec(index = 0)]
    Entry {
        /// Role or skill namespace.
        kind: CatalogKind,
        /// Existing public numeric ID.
        id: u32,
        /// Exact upsert/retained label, without trim or normalization.
        name: String,
        /// Seed/import category or requested for a fresh admin write.
        category: String,
    },
    /// Signed request, whose public name uses the existing trim rule.
    #[codec(index = 1)]
    Request(CreateSkillRequest),
}
/// The API stores this record immutably; core selects only the fixed digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogMetadataRecordV1 {
    /// Deployment generation, fixed before signing.
    pub generation: ProviderInstanceId,
    /// Explicit signed/import/seed source.
    pub provenance: CatalogProvenanceV1,
    /// Original descriptive value and any request role inputs.
    pub content: CatalogContentV1,
}
impl CatalogMetadataRecordV1 {
    /// Exact Rust normalization already used by the native catalog.
    #[must_use]
    pub fn normalized(name: &str) -> String {
        name.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    /// Produce staging data from the original authorized public operation.
    #[must_use]
    pub fn from_command(
        generation: ProviderInstanceId,
        author: AccountId32,
        operation_id: OperationId,
        command: &ProviderCommand,
    ) -> Option<Self> {
        let content = match command {
            ProviderCommand::UpsertCatalogEntry(request) => CatalogContentV1::Entry {
                kind: request.kind,
                id: request.id,
                name: request.name.clone(),
                category: "requested".into(),
            },
            ProviderCommand::CreateSkillRequest(request) => {
                CatalogContentV1::Request(request.clone())
            }
            _ => return None,
        };
        Some(Self {
            generation,
            provenance: CatalogProvenanceV1::Operation {
                operation_id,
                author,
            },
            content,
        })
    }
    /// Verify source and generation; entity/request/role context is checked by the
    /// specific selection owner before rendering or importing.
    #[must_use]
    pub fn matches_source(
        &self,
        generation: ProviderInstanceId,
        author: Option<AccountId32>,
        reference: &CatalogReferenceV1,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        match (&self.provenance, reference) {
            (
                CatalogProvenanceV1::Operation {
                    operation_id,
                    author: origin,
                },
                CatalogReferenceV1::Write(MetadataCommitmentV1 {
                    operation_id: selected,
                    ..
                }),
            ) => *operation_id == *selected && author == Some(*origin),
            (CatalogProvenanceV1::Seed { dataset_version: 1 }, CatalogReferenceV1::Seed(_)) => true,
            _ => false,
        }
    }
    /// SHA256 preimage. Hash on a blocking worker at asynchronous boundaries.
    #[must_use]
    pub fn hash_input(&self) -> Vec<u8> {
        let mut bytes = b"abako/catalog-document/v1\0".to_vec();
        self.encode_to(&mut bytes);
        bytes
    }
}

impl CatalogMetadataRecordV1 {
    /// Render only after the owner separately verifies the selected digest.
    pub fn request_name(
        &self,
        generation: ProviderInstanceId,
        request: &super::SkillRequestCore,
    ) -> Result<&str, crate::ContractError> {
        let invalid = || crate::ContractError::Invalid("catalog request context");
        if !self.matches_source(generation, Some(request.requester), &request.content) {
            return Err(invalid());
        }
        let (name, roles) = match (&self.provenance, &self.content) {
            (CatalogProvenanceV1::Operation { .. }, CatalogContentV1::Request(original)) => {
                (original.name.trim(), &original.role_ids)
            }
            _ => return Err(invalid()),
        };
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: name.into(),
            role_ids: roles.clone(),
        })
        .validate()?;
        if roles != &request.role_ids {
            return Err(invalid());
        }
        Ok(name)
    }
    fn approved_request<'a>(
        id: u32,
        reference: &CatalogReferenceV1,
        requests: &'a [super::SkillRequestCore],
    ) -> Result<&'a super::SkillRequestCore, crate::ContractError> {
        requests
            .iter()
            .find(|request| {
                request.status == crate::SkillRequestStatus::Approved
                    && request.skill_id == Some(id)
                    && request.content == *reference
            })
            .ok_or(crate::ContractError::Invalid("catalog approval context"))
    }
    /// Current name only; category/origin availability is independent presentation.
    pub fn entry_name(
        &self,
        generation: ProviderInstanceId,
        kind: CatalogKind,
        id: u32,
        root: AccountId32,
        reference: &CatalogReferenceV1,
        requests: &[super::SkillRequestCore],
    ) -> Result<&str, crate::ContractError> {
        match &self.content {
            CatalogContentV1::Entry {
                kind: scope,
                id: target,
                name,
                ..
            } if *scope == kind
                && *target == id
                && self.matches_source(generation, Some(root), reference) =>
            {
                ProviderCommand::UpsertCatalogEntry(crate::UpsertCatalogEntryRequest {
                    kind,
                    id,
                    name: name.clone(),
                })
                .validate()?;
                Ok(name)
            }
            CatalogContentV1::Request(_) if kind == CatalogKind::Skill => {
                self.request_name(generation, Self::approved_request(id, reference, requests)?)
            }
            _ => Err(crate::ContractError::Invalid("catalog label context")),
        }
    }
    /// Category is selected by the actual core incarnation, never an enqueue guess.
    pub fn category(
        &self,
        generation: ProviderInstanceId,
        id: u32,
        root: AccountId32,
        reference: &CatalogReferenceV1,
        requests: &[super::SkillRequestCore],
    ) -> Result<&str, crate::ContractError> {
        match &self.content {
            CatalogContentV1::Entry {
                kind: CatalogKind::Skill,
                id: target,
                category,
                ..
            } if *target == id && self.matches_source(generation, Some(root), reference) => {
                if matches!(self.provenance, CatalogProvenanceV1::Operation { .. })
                    && category != "requested"
                {
                    return Err(crate::ContractError::Invalid("catalog origin context"));
                }
                Ok(category)
            }
            CatalogContentV1::Request(_) => {
                self.request_name(generation, Self::approved_request(id, reference, requests)?)?;
                Ok("requested")
            }
            CatalogContentV1::Entry { .. } => {
                Err(crate::ContractError::Invalid("catalog origin context"))
            }
        }
    }
}
