//! Compact catalog facts. Names, categories and normalization belong to the API.
use super::MetadataCommitmentV1;
use crate::{
    AccountId32, CatalogKind, CompensationPolicy, EntityId, PayloadHash, ScorePolicy,
    SkillRequestStatus,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Fixed context reference; seed/import never claim a historical signed write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "Three fixed SCALE variants."
)]
pub enum CatalogReferenceV1 {
    /// Real accepted operation with a context-bound immutable document.
    #[codec(index = 0)]
    Write(MetadataCommitmentV1),
    /// Retained value from the exact owner export.
    #[codec(index = 1)]
    Migration(PayloadHash),
    /// Explicit generation-bound default dataset record.
    #[codec(index = 2)]
    Seed(PayloadHash),
}
impl CatalogReferenceV1 {
    /// Expected immutable catalog-context digest.
    #[must_use]
    pub const fn hash(&self) -> PayloadHash {
        match self {
            Self::Write(value) => value.hash,
            Self::Migration(value) | Self::Seed(value) => *value,
        }
    }
}
/// A catalog label is content, not a qualification or an authority capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEntryCore {
    /// Existing public numeric identity.
    pub id: u32,
    /// Only coordinator role1 is fixed.
    pub fixed: bool,
    /// Current name selection; independent from the skill's origin.
    pub content: CatalogReferenceV1,
}
/// Role references and the actual skill incarnation survive ordinary renames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillMetadataCore {
    /// Preserve ordered role associations and role deletion guards.
    pub role_ids: Vec<u32>,
    /// Creation-selected category document; replaced only on delete/recreate.
    pub origin: CatalogReferenceV1,
}
/// Private snapshot association; the adapter strips the origin in public JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillRoleCore {
    /// Existing numeric skill identity.
    pub skill_id: u32,
    /// Actual contractual role references.
    pub role_ids: Vec<u32>,
    /// Exact incarnation selected during actual creation.
    pub origin: CatalogReferenceV1,
}
/// Compact moderated request lifecycle, retaining receipt/event and RNG identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillRequestCore {
    /// Actual core-assigned request identity.
    pub request_id: EntityId,
    /// Verified requester used by privacy and event recipients.
    pub requester: AccountId32,
    /// Immutable request content; approval may reuse this for the catalog label.
    pub content: CatalogReferenceV1,
    /// Roles checked at request creation and again at approval.
    pub role_ids: Vec<u32>,
    /// One-time authoritative decision state.
    pub status: SkillRequestStatus,
    /// Actual max-ID+1 allocation; never predicted in API staging.
    pub skill_id: Option<u32>,
}
/// Catalog core query. Request history retains its existing scoped endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogCore {
    /// Current role identities/capabilities and label references.
    pub roles: Vec<CatalogEntryCore>,
    /// Current skill identities and label references.
    pub skills: Vec<CatalogEntryCore>,
    /// Compact role-reference and incarnation facts.
    pub skill_roles: Vec<SkillRoleCore>,
    /// Current voting policy stays independently enforceable.
    pub score_policy: ScorePolicy,
    /// Current allocation policy stays independently enforceable.
    pub compensation_policy: CompensationPolicy,
}
impl CatalogCore {
    /// Select an existing ID, without resolving descriptive content.
    #[must_use]
    pub fn entry(&self, kind: CatalogKind, id: u32) -> Option<&CatalogEntryCore> {
        let entries = match kind {
            CatalogKind::Role => &self.roles,
            CatalogKind::Skill => &self.skills,
        };
        entries.iter().find(|entry| entry.id == id)
    }
}

/// Preserve the existing nonempty/unique/nonzero request role-ID admission.
pub fn validate_skill_request_roles(role_ids: &[u32]) -> Result<(), crate::ContractError> {
    if role_ids.is_empty() {
        return Err(crate::ContractError::Invalid("skill role IDs"));
    }
    crate::catalog_ids(role_ids, "skill role IDs")
}
