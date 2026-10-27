//! Independent replay of native Directory traces against production ink WASM.
//! Accounts/IDs are explicit fixtures; signed command routing remains pending.
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
fn call(origin: &Account, directory: &Account, data: Vec<u8>) -> ExecReturnValue {
    Contracts::bare_call(
        origin.clone(),
        directory.clone(),
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
fn read<T: Decode>(directory: &Account, caller: u8, name: &str, args: impl Encode) -> T {
    let value = call(&account(caller), directory, input(name, args));
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
    serde_json::from_str(include_str!(
        "../../../services/native-provider/src/domain/fixtures/directory-parity.json"
    ))
    .unwrap()
}
fn setup(seed: &Catalog, valid_code: bool) -> (sp_io::TestExternalities, Account) {
    let bundles = Bundles::pinned().unwrap();
    let mut ext = new_externalities((1..=4).map(account).collect()).unwrap();
    let directory = ext.execute_with(|| {
        let uploaded = Contracts::bare_upload_code(
            account(1),
            bundles.code("calendar").unwrap().to_vec(),
            None,
            Determinism::Enforced,
        )
        .unwrap();
        let hash = if valid_code {
            uploaded.code_hash
        } else {
            H256::repeat_byte(99)
        };
        let created = Contracts::bare_instantiate(
            account(1),
            0,
            GAS,
            None,
            Code::Upload(bundles.code("directory").unwrap().to_vec()),
            input("new", (hash, account(4), account(4), seed)),
            b"directory".to_vec(),
            DebugInfo::Skip,
            CollectEvents::Skip,
        )
        .result
        .unwrap();
        assert!(!created.result.did_revert(), "{:?}", created.result);
        created.account_id
    });
    (ext, directory)
}
fn projection(directory: &Account) -> Projection {
    let (roles, skills, associations): Catalog = read(directory, 1, "catalog", ());
    (
        roles,
        skills,
        associations,
        read(directory, 1, "workers", ()),
        read(directory, 1, "skill_requests", ()),
    )
}
fn digest(state: &Projection) -> String {
    format!("{:x}", H256::from(sha2_256(&state.encode())))
}
fn command_input(command: ProviderCommand, id: EntityId, origin: AccountId32) -> Vec<u8> {
    let command = if matches!(
        command,
        ProviderCommand::RegisterWorker(_)
            | ProviderCommand::UpsertCatalogEntry(_)
            | ProviderCommand::CreateSkillRequest(_)
    ) {
        let call = UnsignedContractCallV1::new(
            ProviderInstanceId::from_bytes([0; 16]),
            OperationId::from_bytes([0; 16]),
            origin,
            0,
            UnixSeconds::new(u64::MAX),
            command,
        )
        .unwrap();
        native_provider::creation::compact_creation(&call)
            .or_else(|| native_provider::catalog_content::compact_catalog(&call))
            .unwrap()
    } else {
        command
    };
    match command {
        ProviderCommand::RegisterWorkerCore(request) => {
            input("register_worker_core", (request, id))
        }
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::UpsertCatalogEntryCore { kind, id, content } => {
            input("upsert_catalog_entry_core", (kind, id, content))
        }
        ProviderCommand::DeleteCatalogEntry(request) => input("delete_catalog_entry", request),
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            input("create_skill_request_core", (role_ids, content, id))
        }
        ProviderCommand::DecideSkillRequest(request) => input("decide_skill_request", request),
        _ => panic!("incomplete Directory coverage"),
    }
}

#[test]
fn canonical_directory_trace_cross_calls_and_restart() {
    let fixture = fixture();
    assert_eq!(fixture["schemaVersion"], 1);
    let initial: Projection = serde_json::from_value(fixture["initial"].clone()).unwrap();
    let (mut ext, directory) = setup(
        &(initial.0.clone(), initial.1.clone(), initial.2.clone()),
        true,
    );
    ext.execute_with(|| assert_eq!(projection(&directory), initial));
    for (index, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        ext.execute_with(|| {
            let origin: AccountId32 = serde_json::from_value(step["origin"].clone()).unwrap();
            let command = serde_json::from_value(step["command"].clone()).unwrap();
            // These are captured IDs, not a claimed RNG/signed bridge implementation.
            let id: EntityId = serde_json::from_value(step["capturedId"].clone()).unwrap();
            let value = call(
                &Account::new(origin.into_bytes()),
                &directory,
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
                digest(&projection(&directory)),
                step["stateDigest"],
                "step {index}: {step}"
            );
        });
        if index == 9 || index == 26 {
            let checkpoint = Checkpoint::capture(ext).unwrap().to_bytes().unwrap();
            ext = Checkpoint::decode(&checkpoint).unwrap().restore().unwrap();
        }
    }
    ext.execute_with(|| assert_eq!(json!(projection(&directory)), fixture["final"]));
}

#[test]
fn failed_child_creation_leaves_no_worker_or_calendar() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let seed = (initial.0, initial.1, initial.2);
    {
        let (mut ext, directory) = setup(&seed, false);
        ext.execute_with(|| {
            let before = digest(&projection(&directory));
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
                    overrides: vec![],
                },
            };
            let value = call(
                &account(2),
                &directory,
                input(
                    "register_worker_core",
                    (request, EntityId::from_bytes([8; 16])),
                ),
            );
            assert!(value.did_revert());
            assert_eq!(
                outcome::<Effect>(&value).unwrap_err(),
                "calendar_instantiation_failed"
            );
            assert_eq!(
                read::<Option<Account>>(&directory, 1, "calendar_address", account(2)),
                None
            );
            assert_eq!(digest(&projection(&directory)), before);
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
fn write(directory: &Account, caller: u8, name: &str, args: impl Encode) -> Effect {
    let value = call(&account(caller), directory, input(name, args));
    assert!(!value.did_revert(), "{name}: {value:?}");
    outcome(&value).unwrap()
}
fn reject<T: Decode>(directory: &Account, caller: u8, name: &str, args: impl Encode, code: &str) {
    let value = call(&account(caller), directory, input(name, args));
    assert!(value.did_revert());
    assert!(matches!(outcome::<T>(&value),Err(error) if error == code));
}

#[test]
fn request_visibility_reference_guards_and_nested_calendar_permissions() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let (mut ext, directory) = setup(&(initial.0, initial.1, initial.2), true);
    ext.execute_with(|| {
        write(
            &directory,
            2,
            "register_worker_core",
            (registration(), [1_u8; 16]),
        );
        let before = digest(&projection(&directory));
        reject::<Effect>(
            &directory,
            3,
            "register_worker_core",
            (registration(), [1_u8; 16]),
            "invalid_worker_state",
        );
        assert_eq!(digest(&projection(&directory)), before);
        write(
            &directory,
            3,
            "register_worker_core",
            (registration(), [2_u8; 16]),
        );
        write(
            &directory,
            2,
            "create_skill_request_core",
            (vec![3_u32], commitment(5), [5_u8; 16]),
        );
        write(
            &directory,
            3,
            "create_skill_request_core",
            (vec![2_u32], commitment(6), [6_u8; 16]),
        );
        for (caller, expected) in [(1, 2), (2, 1), (3, 1), (4, 0)] {
            let requests: Vec<SkillRequestCore> = read(&directory, caller, "skill_requests", ());
            assert_eq!(requests.len(), expected);
            assert!(
                requests
                    .iter()
                    .all(|r| caller == 1 || r.requester.into_bytes() == [caller; 32])
            );
        }
        write(
            &directory,
            1,
            "upsert_catalog_entry_core",
            (CatalogKind::Role, 90_u32, commitment(90)),
        );
        write(
            &directory,
            1,
            "upsert_catalog_entry_core",
            (CatalogKind::Skill, 80_u32, commitment(80)),
        );
        let refs = Some(Qualifications {
            role_ids: vec![90],
            skill_ids: vec![80],
        });
        for caller in [1, 2, 3] {
            reject::<()>(
                &directory,
                caller,
                "set_proposal_references",
                ([9_u8; 16], &refs),
                "reference_controller_required",
            );
        }
        let value = call(
            &account(4),
            &directory,
            input("set_proposal_references", ([9_u8; 16], &refs)),
        );
        assert_eq!(outcome::<()>(&value), Ok(()));
        for kind in [CatalogKind::Role, CatalogKind::Skill] {
            reject::<Effect>(
                &directory,
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
            &directory,
            input(
                "set_proposal_references",
                ([9_u8; 16], Option::<Qualifications>::None),
            ),
        );
        assert_eq!(outcome::<()>(&value), Ok(()));
        write(
            &directory,
            1,
            "delete_catalog_entry",
            (CatalogKind::Skill, 80_u32),
        );
        write(
            &directory,
            1,
            "delete_catalog_entry",
            (CatalogKind::Role, 90_u32),
        );

        let calendar: Option<Account> = read(&directory, 2, "calendar_address", account(2));
        let calendar = calendar.unwrap();
        let bundles = Bundles::pinned().unwrap();
        assert_eq!(
            Contracts::code_hash(&calendar),
            Some(H256::from(blake2_256(bundles.code("calendar").unwrap())))
        );
        // Directory is an actual contract caller here, not the root fixture account.
        reject::<()>(
            &calendar,
            1,
            "set_calendar",
            registration().calendar,
            "calendar_owner_required",
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
            &calendar,
            4,
            "reserve",
            (
                WeekWindow::new(week, week).unwrap(),
                Minutes::new(60),
                reference.clone(),
            ),
            "reservation_controller_required",
        );
        let value = call(
            &account(4),
            &directory,
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
        let reserved: CalendarView = read(&calendar, 2, "view", ());
        write(&directory, 1, "promote_coordinator", account(2));
        write(&directory, 2, "set_worker_mode", WorkerMode::Coordinator);
        assert_eq!(read::<CalendarView>(&calendar, 2, "view", ()), reserved);
        let before = digest(&projection(&directory));
        reject::<Effect>(
            &directory,
            2,
            "set_calendar",
            CalendarDefinition {
                default_weekly_minutes: Minutes::new(59),
                overrides: vec![],
            },
            "calendar_overcommitted",
        );
        assert_eq!(digest(&projection(&directory)), before);
        write(
            &directory,
            2,
            "set_calendar",
            CalendarDefinition {
                default_weekly_minutes: Minutes::new(100),
                overrides: vec![],
            },
        );
        assert_eq!(
            read::<CalendarView>(&calendar, 2, "view", ()).reservations,
            reserved.reservations
        );
    });
}

#[test]
fn parent_storage_failure_discards_tentative_child_instance() {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    let (mut ext, directory) = setup(&(initial.0, initial.1, initial.2), true);
    ext.execute_with(|| {
        let request = registration();
        // Measure the successful parent/child transaction, discard it, then
        // exhaust gas at its final boundary rather than relying on packed storage.
        let id=[8_u8;16];
        let code_hash=H256::from(blake2_256(Bundles::pinned().unwrap().code("calendar").unwrap()));
        let child=Contracts::contract_address(&directory,&code_hash,&input("new",(account(2),directory.clone(),id,&request.calendar)),&id);
        let data=input("register_worker_core",(request,id));
        let before=digest(&projection(&directory));
        sp_io::storage::start_transaction();
        let completed=Contracts::bare_call(account(2),directory.clone(),0,GAS,None,data.clone(),DebugInfo::Skip,CollectEvents::Skip,Determinism::Enforced);
        assert!(!completed.result.unwrap().did_revert());
        assert!(Contracts::code_hash(&child).is_some());
        sp_io::storage::rollback_transaction();
        assert_eq!(Contracts::code_hash(&child),None);
        let gas=frame_support::weights::Weight::from_parts(completed.gas_consumed.ref_time().checked_sub(1).unwrap(),GAS.proof_size());
        let result=Contracts::bare_call(account(2),directory.clone(),0,gas,None,data,DebugInfo::Skip,CollectEvents::Skip,Determinism::Enforced).result;
        assert!(matches!(result,Err(error) if error==pallet_contracts::Error::<contract_provider::Runtime>::OutOfGas.into()));
        assert_eq!(Contracts::code_hash(&child),None);
        assert_eq!(read::<Option<Account>>(&directory,1,"calendar_address",account(2)),None);
        assert_eq!(digest(&projection(&directory)),before);
    });
}

fn commitment(n: u8) -> protocol::provider_core::MetadataCommitmentV1 {
    protocol::provider_core::MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([n; 16]),
        hash: PayloadHash::from_bytes([n; 32]),
    }
}
