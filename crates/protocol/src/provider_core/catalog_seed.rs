//! Explicit default catalog dataset, shared by host provisioning and API content.
use super::{CatalogContentV1, CatalogMetadataRecordV1, CatalogProvenanceV1};
use crate::{CatalogKind, ProviderInstanceId};

/// Existing default label and numeric seed facts; not a contract storage model.
pub struct CatalogSeedEntryV1 {
    /// Role or skill namespace.
    pub kind: CatalogKind,
    /// Stable existing public ID.
    pub id: u32,
    /// Exact default human label.
    pub name: &'static str,
    /// Exact retained category; empty for role labels.
    pub category: &'static str,
    /// Existing qualification references for a default skill.
    pub role_ids: &'static [u32],
}
impl CatalogSeedEntryV1 {
    /// A seed value is explicit and generation-bound, never an old signature.
    #[must_use]
    pub fn record(&self, generation: ProviderInstanceId) -> CatalogMetadataRecordV1 {
        CatalogMetadataRecordV1 {
            generation,
            provenance: CatalogProvenanceV1::Seed { dataset_version: 1 },
            content: CatalogContentV1::Entry {
                kind: self.kind,
                id: self.id,
                name: self.name.into(),
                category: self.category.into(),
            },
        }
    }
}
/// Retain all 9 roles and 33 skills from the previous seed without duplicate copies.
#[must_use]
pub fn catalog_seed_v1() -> Vec<CatalogSeedEntryV1> {
    let mut entries = Vec::new();
    for (index, name) in [
        "coordinator",
        "frontend",
        "backend",
        "fullstack",
        "designer",
        "qa",
        "architect",
        "embedded",
        "devops",
    ]
    .into_iter()
    .enumerate()
    {
        if let Ok(id) = u32::try_from(index + 1) {
            entries.push(CatalogSeedEntryV1 {
                kind: CatalogKind::Role,
                id,
                name,
                category: "",
                role_ids: &[],
            });
        }
    }
    let definitions: [(&str, &[u32]); 33] = [
        ("rust", &[3, 7, 8]),
        ("solidity", &[3, 7]),
        ("ink", &[3, 7]),
        ("substrate", &[3, 7]),
        ("typescript", &[2, 3, 4]),
        ("javascript", &[2, 3, 4]),
        ("node.js", &[3, 4]),
        ("react", &[2, 4]),
        ("next.js", &[2, 4]),
        ("vue", &[2, 4]),
        ("postgresql", &[3, 4, 7, 9]),
        ("sqlite", &[3, 4, 8]),
        ("docker", &[3, 7, 9]),
        ("kubernetes", &[7, 9]),
        ("aws", &[7, 9]),
        ("graphql", &[2, 3, 4, 7]),
        ("rest api", &[2, 3, 4, 7]),
        ("web3", &[2, 3, 4, 7]),
        ("smart contract auditing", &[3, 6, 7]),
        ("automated testing", &[2, 3, 4, 6, 8, 9]),
        ("ui/ux", &[2, 4, 5]),
        ("figma", &[2, 5]),
        ("react native", &[2, 4]),
        ("communication", &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
        ("leadership", &[1, 7]),
        ("mentoring", &[1, 7]),
        ("problem solving", &[2, 3, 4, 5, 6, 7, 8, 9]),
        ("stakeholder management", &[1, 5, 7]),
        ("facilitation", &[1, 5, 7]),
        ("technical writing", &[1, 3, 6, 7, 8, 9]),
        ("teamwork", &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
        ("adaptability", &[2, 3, 4, 5, 6, 7, 8, 9]),
        ("time management", &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
    ];
    for (index, (name, role_ids)) in definitions.into_iter().enumerate() {
        if let Ok(id) = u32::try_from(index + 1) {
            entries.push(CatalogSeedEntryV1 {
                kind: CatalogKind::Skill,
                id,
                name,
                category: if id <= 23 { "software" } else { "soft" },
                role_ids,
            });
        }
    }
    entries
}
