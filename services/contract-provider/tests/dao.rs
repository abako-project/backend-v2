//! Dao behavior scenarios against production ink WASM.
//! Explicit account/ID fixtures isolate contract behavior from signed transport.
use contract_provider::{
    Account, Contracts, GAS, bundle::Bundles, checkpoint::Checkpoint, new_externalities,
};
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism, ExecReturnValue};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use protocol::provider_core::{CatalogEntryCore, SkillRequestCore, SkillRoleCore, WorkerCore};
use protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sp_core::{
    H256,
    hashing::{blake2_256, sha2_256},
};

type Projection = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
    Vec<WorkerCore>,
    Vec<SkillRequestCore>,
);
type Catalog = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
);
#[derive(Debug, Encode, Decode, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Effect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<AccountId32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocated_task_id: Option<u32>,
}

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
fn call(origin: &Account, dao: &Account, data: Vec<u8>) -> ExecReturnValue {
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
fn read<T: Decode>(dao: &Account, caller: u8, name: &str, args: impl Encode) -> T {
    let value = call(&account(caller), dao, input(name, args));
    assert!(!value.did_revert());
    Result::<T, u8>::decode_all(&mut &value.data[..])
        .unwrap()
        .unwrap()
}
fn outcome<T: Decode>(value: &ExecReturnValue) -> Result<T, String> {
    Result::<Result<T, String>, u8>::decode_all(&mut &value.data[..])
        .unwrap()
        .unwrap()
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/directory.json")).unwrap()
}
fn setup(seed: &Catalog) -> (sp_io::TestExternalities, Account) {
    let bundles = Bundles::pinned().unwrap();
    let mut ext = new_externalities((1..=4).map(account).collect()).unwrap();
    let dao = ext.execute_with(|| {
        let created = Contracts::bare_instantiate(
            account(1),
            0,
            GAS,
            None,
            Code::Upload(bundles.code("dao").unwrap().to_vec()),
            input("new", (account(4), account(4), seed)),
            b"dao".to_vec(),
            DebugInfo::Skip,
            CollectEvents::Skip,
        )
        .result
        .unwrap();
        assert!(!created.result.did_revert(), "{:?}", created.result);
        created.account_id
    });
    (ext, dao)
}
fn projection(dao: &Account) -> Projection {
    let (roles, skills, associations): Catalog = read(dao, 1, "catalog", ());
    (
        roles,
        skills,
        associations,
        read(dao, 1, "workers", ()),
        read(dao, 1, "skill_requests", ()),
    )
}
fn digest(state: &Projection) -> String {
    format!("{:x}", H256::from(sha2_256(&state.encode())))
}
fn command_input(command: ProviderCommand, id: EntityId, _origin: AccountId32) -> Vec<u8> {
    match command {
        ProviderCommand::RegisterWorkerCore(request) => input("register_worker", (request, id)),
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::UpsertCatalogEntryCore { kind, id, content } => {
            input("upsert_catalog_entry", (kind, id, content))
        }
        ProviderCommand::DeleteCatalogEntry(request) => input("delete_catalog_entry", request),
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            input("create_skill_request", (role_ids, content, id))
        }
        ProviderCommand::DecideSkillRequest(request) => input("decide_skill_request", request),
        _ => panic!("incomplete Dao coverage"),
    }
}

#[test]
fn canonical_dao_trace_and_restart() {
    let fixture = fixture();
    assert_eq!(fixture["schemaVersion"], 1);
    let initial: Projection = serde_json::from_value(fixture["initial"].clone()).unwrap();
    let (mut ext, dao) = setup(&(initial.0.clone(), initial.1.clone(), initial.2.clone()));
    ext.execute_with(|| assert_eq!(projection(&dao), initial));
    for (index, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        ext.execute_with(|| {
            let origin: AccountId32 = serde_json::from_value(step["origin"].clone()).unwrap();
            let command = serde_json::from_value(step["command"].clone()).unwrap();
            // These are captured IDs, not a claimed RNG/signed bridge implementation.
            let id: EntityId = serde_json::from_value(step["capturedId"].clone()).unwrap();
            let value = call(
                &Account::new(origin.into_bytes()),
                &dao,
                command_input(command, id, origin),
            );
            let observed = match outcome::<Effect>(&value) {
                Ok(effect) => {
                    assert!(!value.did_revert());
                    json!({"effect":effect})
                }
                Err(error) => {
                    assert!(value.did_revert());
                    json!({"error":error})
                }
            };
            assert_eq!(observed, step["outcome"], "step {index}: {step}");
            assert_eq!(
                digest(&projection(&dao)),
                step["stateDigest"],
                "step {index}: {step}"
            );
        });
        if index == 9 || index == 26 {
            let checkpoint = Checkpoint::capture(ext).unwrap().to_bytes().unwrap();
            ext = Checkpoint::decode(&checkpoint).unwrap().restore().unwrap();
        }
    }
}

#[test]
fn invalid_calendar_leaves_no_worker_or_capacity() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let seed = (initial.0, initial.1, initial.2);
    {
        let (mut ext, dao) = setup(&seed);
        ext.execute_with(|| {
            let before = digest(&projection(&dao));
            let request = protocol::provider_core::RegisterWorkerCoreRequest {
                content: protocol::provider_core::MetadataCommitmentV1 {
                    operation_id: OperationId::from_bytes([0; 16]),
                    hash: PayloadHash::from_bytes([9; 32]),
                },
                qualifications: Qualifications {
                    role_ids: vec![2],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(60),
                    overrides: vec![
                        WeekOverride {
                            week: Week::new(2026, 1).unwrap(),
                            capacity: Minutes::new(60)
                        };
                        2
                    ],
                },
            };
            let value = call(
                &account(2),
                &dao,
                input("register_worker", (request, EntityId::from_bytes([8; 16]))),
            );
            assert!(value.did_revert());
            assert_eq!(outcome::<Effect>(&value).unwrap_err(), "invalid_contract");
            assert_eq!(
                read::<Option<WorkerCore>>(&dao, 1, "worker", account(2)),
                None
            );
            assert_eq!(digest(&projection(&dao)), before);
        });
    }
}

fn registration() -> protocol::provider_core::RegisterWorkerCoreRequest {
    protocol::provider_core::RegisterWorkerCoreRequest {
        content: protocol::provider_core::MetadataCommitmentV1 {
            operation_id: OperationId::from_bytes([0; 16]),
            hash: PayloadHash::from_bytes([9; 32]),
        },
        qualifications: Qualifications {
            role_ids: vec![2],
            skill_ids: vec![1],
        },
        calendar: CalendarDefinition {
            default_weekly_minutes: Minutes::new(600),
            overrides: vec![],
        },
    }
}
fn write(dao: &Account, caller: u8, name: &str, args: impl Encode) -> Effect {
    let value = call(&account(caller), dao, input(name, args));
    assert!(!value.did_revert(), "{name}: {value:?}");
    outcome(&value).unwrap()
}
fn reject<T: Decode>(dao: &Account, caller: u8, name: &str, args: impl Encode, code: &str) {
    let value = call(&account(caller), dao, input(name, args));
    assert!(value.did_revert());
    assert!(matches!(outcome::<T>(&value),Err(error) if error == code));
}

#[test]
fn request_visibility_reference_guards_and_calendar_permissions() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let (mut ext, dao) = setup(&(initial.0, initial.1, initial.2));
    ext.execute_with(|| {
        write(&dao, 2, "register_worker", (registration(), [1_u8; 16]));
        let before = digest(&projection(&dao));
        reject::<Effect>(
            &dao,
            3,
            "register_worker",
            (registration(), [1_u8; 16]),
            "invalid_worker_state",
        );
        assert_eq!(digest(&projection(&dao)), before);
        write(&dao, 3, "register_worker", (registration(), [2_u8; 16]));
        write(
            &dao,
            2,
            "create_skill_request",
            (vec![3_u32], commitment(5), [5_u8; 16]),
        );
        write(
            &dao,
            3,
            "create_skill_request",
            (vec![2_u32], commitment(6), [6_u8; 16]),
        );
        for (caller, expected) in [(1, 2), (2, 1), (3, 1), (4, 0)] {
            let requests: Vec<SkillRequestCore> = read(&dao, caller, "skill_requests", ());
            assert_eq!(requests.len(), expected);
            assert!(
                requests
                    .iter()
                    .all(|r| caller == 1 || r.requester.into_bytes() == [caller; 32])
            );
        }
        write(
            &dao,
            1,
            "upsert_catalog_entry",
            (CatalogKind::Role, 90_u32, commitment(90)),
        );
        write(
            &dao,
            1,
            "upsert_catalog_entry",
            (CatalogKind::Skill, 80_u32, commitment(80)),
        );
        let refs = Some(Qualifications {
            role_ids: vec![90],
            skill_ids: vec![80],
        });
        for caller in [1, 2, 3] {
            reject::<()>(
                &dao,
                caller,
                "set_proposal_references",
                ([9_u8; 16], &refs),
                "reference_controller_required",
            );
        }
        let value = call(
            &account(4),
            &dao,
            input("set_proposal_references", ([9_u8; 16], &refs)),
        );
        assert_eq!(outcome::<()>(&value), Ok(()));
        for kind in [CatalogKind::Role, CatalogKind::Skill] {
            reject::<Effect>(
                &dao,
                1,
                "delete_catalog_entry",
                (
                    kind,
                    if kind == CatalogKind::Role {
                        90_u32
                    } else {
                        80
                    },
                ),
                "catalog_entry_in_use",
            );
        }
        let value = call(
            &account(4),
            &dao,
            input(
                "set_proposal_references",
                ([9_u8; 16], Option::<Qualifications>::None),
            ),
        );
        assert_eq!(outcome::<()>(&value), Ok(()));
        write(
            &dao,
            1,
            "delete_catalog_entry",
            (CatalogKind::Skill, 80_u32),
        );
        write(&dao, 1, "delete_catalog_entry", (CatalogKind::Role, 90_u32));

        reject::<Effect>(
            &dao,
            1,
            "set_calendar",
            registration().calendar,
            "worker_not_found",
        );
        let week = Week::new(2026, 1).unwrap();
        let reference = ReservationView {
            project_id: EntityId::from_bytes([10; 16]),
            milestone_id: Some(EntityId::from_bytes([11; 16])),
            requirement_key: Some(7),
            week,
            minutes: Minutes::ZERO,
        };
        reject::<()>(
            &dao,
            2,
            "reserve_calendar",
            (
                account(2),
                WeekWindow::new(week, week).unwrap(),
                Minutes::new(60),
                reference.clone(),
            ),
            "reservation_controller_required",
        );
        let value = call(
            &account(4),
            &dao,
            input(
                "reserve_calendar",
                (
                    account(2),
                    WeekWindow::new(week, week).unwrap(),
                    Minutes::new(60),
                    reference,
                ),
            ),
        );
        assert_eq!(outcome::<()>(&value), Ok(()));
        let reserved: CalendarView =
            read::<Result<CalendarView, String>>(&dao, 2, "calendar", account(2)).unwrap();
        write(&dao, 1, "promote_coordinator", account(2));
        write(&dao, 2, "set_worker_mode", WorkerMode::Coordinator);
        assert_eq!(
            read::<Result<CalendarView, String>>(&dao, 2, "calendar", account(2)).unwrap(),
            reserved
        );
        let before = digest(&projection(&dao));
        reject::<Effect>(
            &dao,
            2,
            "set_calendar",
            CalendarDefinition {
                default_weekly_minutes: Minutes::new(59),
                overrides: vec![],
            },
            "calendar_overcommitted",
        );
        assert_eq!(digest(&projection(&dao)), before);
        write(
            &dao,
            2,
            "set_calendar",
            CalendarDefinition {
                default_weekly_minutes: Minutes::new(100),
                overrides: vec![],
            },
        );
        assert_eq!(
            read::<Result<CalendarView, String>>(&dao, 2, "calendar", account(2))
                .unwrap()
                .reservations,
            reserved.reservations
        );
    });
}

#[test]
fn registration_gas_failure_discards_worker_and_calendar_records() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let (mut ext, dao) = setup(&(initial.0, initial.1, initial.2));
    ext.execute_with(|| {
        let request = registration();
        // Measure a successful registration, discard it, then
        // exhaust gas at its final boundary rather than relying on packed storage.
        let id=[8_u8;16];
        let calendar_key=(0x4341_4c01u32,account(2)).encode();
        let data=input("register_worker",(request,id));
        let before=digest(&projection(&dao));
        sp_io::storage::start_transaction();
        let completed=Contracts::bare_call(account(2),dao.clone(),0,GAS,None,data.clone(),DebugInfo::Skip,CollectEvents::Skip,Determinism::Enforced);
        assert!(!completed.result.unwrap().did_revert());
        assert!(Contracts::get_storage(dao.clone(),calendar_key.clone()).unwrap().is_some());
        sp_io::storage::rollback_transaction();
        assert_eq!(Contracts::get_storage(dao.clone(),calendar_key.clone()).unwrap(),None);
        let gas=frame_support::weights::Weight::from_parts(completed.gas_consumed.ref_time().checked_sub(1).unwrap(),GAS.proof_size());
        let result=Contracts::bare_call(account(2),dao.clone(),0,gas,None,data,DebugInfo::Skip,CollectEvents::Skip,Determinism::Enforced).result;
        assert!(matches!(result,Err(error) if error==pallet_contracts::Error::<contract_provider::Runtime>::OutOfGas.into()));
        assert_eq!(Contracts::get_storage(dao.clone(),calendar_key.clone()).unwrap(),None);
        assert_eq!(read::<Option<WorkerCore>>(&dao,1,"worker",account(2)),None);
        assert_eq!(digest(&projection(&dao)),before);
    });
}

fn commitment(n: u8) -> protocol::provider_core::MetadataCommitmentV1 {
    protocol::provider_core::MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([n; 16]),
        hash: PayloadHash::from_bytes([n; 32]),
    }
}
