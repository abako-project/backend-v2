//! Fixed local deployment inputs; genesis uses the ordinary contract constructors.
use super::*;
use protocol::{
    CatalogKind, PayloadHash,
    provider_core::{CatalogEntryCore, CatalogReferenceV1, SkillRoleCore, catalog_seed_v1},
};

#[derive(Clone)]
pub struct Configuration {
    pub root: AccountId32,
    pub mock_funding: bool,
    pub gas_limit: Weight,
}
impl Configuration {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if !self.gas_limit.all_lte(crate::EXECUTION_GAS)
            || self.root == AccountId32::from_bytes([0; 32])
        {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    pub(super) fn mode(&self) -> Mode {
        Mode::Contracts {
            root: self.root,
            gas: self.gas_limit,
            configuration: H256::from(sp_core::hashing::blake2_256(
                &(
                    b"contract-provider",
                    self.root,
                    self.mock_funding,
                    self.gas_limit.ref_time(),
                    self.gas_limit.proof_size(),
                )
                    .encode(),
            )),
        }
    }
    pub(super) fn initialize(
        self,
        bundles: &Bundles,
    ) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
        let generation = Shell::new(self.root)?.info.provider_instance_id;
        let mut catalog = (Vec::new(), Vec::new(), Vec::new());
        for seed in catalog_seed_v1() {
            let content = CatalogReferenceV1::Seed(PayloadHash::from_bytes(
                sp_core::hashing::sha2_256(&seed.record(generation).hash_input()),
            ));
            let entry = CatalogEntryCore {
                id: seed.id,
                fixed: seed.kind == CatalogKind::Role && seed.id == 1,
                content: content.clone(),
            };
            match seed.kind {
                CatalogKind::Role => catalog.0.push(entry),
                CatalogKind::Skill => {
                    catalog.1.push(entry);
                    catalog.2.push(SkillRoleCore {
                        skill_id: seed.id,
                        role_ids: seed.role_ids.to_vec(),
                        origin: content,
                    });
                }
            }
        }
        directory::initialize(bundles, self, catalog, generation)
    }
}
