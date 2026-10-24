//! Host-side numeric genesis and explicit API seed commitments; no name storage.
use generated_contracts::{
    CatalogKind, ProviderInstanceId,
    provider_core::{CatalogEntryCore, CatalogReferenceV1, SkillMetadataCore, catalog_seed_v1},
};
use std::collections::BTreeMap;

pub(crate) use generated_contracts::provider_core::SkillMetadataCore as SkillMetadata;

pub(crate) fn coordinator(generation: ProviderInstanceId) -> CatalogEntryCore {
    let entry = catalog_seed_v1().remove(0);
    CatalogEntryCore {
        id: entry.id,
        fixed: true,
        content: CatalogReferenceV1::Seed(crate::migration::source_hash(
            &entry.record(generation).hash_input(),
        )),
    }
}
#[cfg(feature = "mock-seed")]
pub(crate) fn seed(
    generation: ProviderInstanceId,
    roles: &mut BTreeMap<u32, CatalogEntryCore>,
    skills: &mut BTreeMap<u32, CatalogEntryCore>,
    metadata: &mut BTreeMap<u32, SkillMetadataCore>,
) {
    for entry in catalog_seed_v1() {
        let content = CatalogReferenceV1::Seed(crate::migration::source_hash(
            &entry.record(generation).hash_input(),
        ));
        let record = CatalogEntryCore {
            id: entry.id,
            fixed: entry.kind == CatalogKind::Role && entry.id == 1,
            content: content.clone(),
        };
        match entry.kind {
            CatalogKind::Role => {
                roles.entry(entry.id).or_insert(record);
            }
            CatalogKind::Skill => {
                skills.entry(entry.id).or_insert(record);
                metadata.entry(entry.id).or_insert(SkillMetadataCore {
                    role_ids: entry.role_ids.to_vec(),
                    origin: content,
                });
            }
        }
    }
}
