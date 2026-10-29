//! Shared direct-WASM test calls and fixed catalog expectations.
use contract_provider::{Account, Contracts, GAS};
use pallet_contracts::{CollectEvents, DebugInfo, Determinism, ExecReturnValue};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use protocol::provider_core::{CatalogEntryCore, SkillRequestCore, SkillRoleCore, WorkerCore};
use protocol::*;
use serde::{Deserialize, Serialize};
use sp_core::{
    H256,
    hashing::{blake2_256, sha2_256},
};

pub type Projection = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
    Vec<WorkerCore>,
    Vec<SkillRequestCore>,
);
pub type Catalog = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
);
#[derive(Debug, Encode, Decode, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<AccountId32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocated_task_id: Option<u32>,
}

pub fn account(n: u8) -> Account {
    Account::new([n; 32])
}
pub fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
pub fn call(origin: &Account, dao: &Account, data: Vec<u8>) -> ExecReturnValue {
    Contracts::bare_call(
        origin.clone(),
        dao.clone(),
        0,
        GAS,
        None,
        data,
        DebugInfo::Skip,
        CollectEvents::Skip,
        Determinism::Enforced,
    )
    .result
    .unwrap()
}
pub fn read<T: Decode>(dao: &Account, caller: u8, name: &str, args: impl Encode) -> T {
    let value = call(&account(caller), dao, input(name, args));
    assert!(!value.did_revert());
    Result::<T, u8>::decode_all(&mut &value.data[..])
        .unwrap()
        .unwrap()
}
pub fn projection(dao: &Account) -> Projection {
    let (roles, skills, associations): Catalog = read(dao, 1, "catalog", ());
    (
        roles,
        skills,
        associations,
        read(dao, 1, "workers", ()),
        read(dao, 1, "skill_requests", ()),
    )
}
pub fn digest(state: &Projection) -> String {
    format!("{:x}", H256::from(sha2_256(&state.encode())))
}
pub fn initial() -> Projection {
    serde_json::from_str(include_str!("../fixtures/catalog.json")).unwrap()
}
pub fn catalog() -> Catalog {
    let initial = initial();
    (initial.0, initial.1, initial.2)
}
