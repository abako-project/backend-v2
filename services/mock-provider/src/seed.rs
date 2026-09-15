#[cfg(feature = "mock-seed")]
use std::collections::BTreeMap;

#[cfg(feature = "mock-seed")]
use generated_contracts::CatalogEntry;
use serde::{Deserialize, Serialize};

/// Retained seed metadata is not used to constrain skill matching by role.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SkillMetadata {
    category: String,
    role_ids: Vec<u32>,
}

#[cfg(feature = "mock-seed")]
pub(crate) fn seed(
    roles: &mut BTreeMap<u32, CatalogEntry>,
    skills: &mut BTreeMap<u32, CatalogEntry>,
    metadata: &mut BTreeMap<u32, SkillMetadata>,
) {
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
            roles.entry(id).or_insert_with(|| CatalogEntry {
                id,
                name: name.into(),
                fixed: id == 1,
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
            skills.entry(id).or_insert_with(|| CatalogEntry {
                id,
                name: name.into(),
                fixed: false,
            });
            metadata.entry(id).or_insert_with(|| SkillMetadata {
                category: if id <= 23 { "software" } else { "soft" }.into(),
                role_ids: role_ids.to_vec(),
            });
        }
    }
}
