//! Independent M4 component replay through production contract WASM.
use contract_provider::{
    Account, Contracts, GAS, bundle::Bundles, checkpoint::Checkpoint, new_externalities,
};
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism, ExecReturnValue};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use protocol::provider_core::{CatalogEntryCore, SkillRequestCore, SkillRoleCore, WorkerCore};
use protocol::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
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
#[derive(Debug, Decode, Serialize)]
#[serde(rename_all = "camelCase")]
struct Effect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<AccountId32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocated_task_id: Option<u32>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Action {
    Command {
        origin: AccountId32,
        command: ProviderCommand,
        seed: u8,
    },
    Contributions {
        project_id: EntityId,
        author: AccountId32,
        votes: Vec<(AccountId32, u8, u8, u32)>,
    },
    Coordinator {
        client: AccountId32,
        week: Week,
        seed: u8,
    },
    Assign {
        project_id: EntityId,
        client: AccountId32,
        coordinator: AccountId32,
        milestones: Vec<(EntityId, MilestoneDefinition)>,
        seed: u8,
    },
}
fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
fn call(origin: &Account, directory: &Account, input: Vec<u8>) -> ExecReturnValue {
    Contracts::bare_call(
        origin.clone(),
        directory.clone(),
        0,
        GAS,
        None,
        input,
        DebugInfo::Skip,
        CollectEvents::Skip,
        Determinism::Enforced,
    )
    .result
    .unwrap()
}
fn read<T: Decode>(directory: &Account, name: &str) -> T {
    let value = call(&account(1), directory, input(name, ()));
    assert!(!value.did_revert());
    Result::<T, u8>::decode_all(&mut &value.data[..])
        .unwrap()
        .unwrap()
}
fn decoded<T: Decode>(value: ExecReturnValue) -> Result<T, String> {
    let result = Result::<Result<T, String>, u8>::decode_all(&mut &value.data[..])
        .unwrap()
        .unwrap();
    assert_eq!(value.did_revert(), result.is_err());
    result
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../services/native-provider/src/domain/fixtures/matching-parity.json"
    ))
    .unwrap()
}
fn seed() -> Catalog {
    let initial: Projection = serde_json::from_value(fixture()["initial"].clone()).unwrap();
    (initial.0, initial.1, initial.2)
}
fn setup() -> (sp_io::TestExternalities, Account) {
    let bundles = Bundles::pinned().unwrap();
    let mut ext = new_externalities((1..=100).map(account).collect()).unwrap();
    let directory = ext.execute_with(|| {
        let calendar = Contracts::bare_upload_code(
            account(1),
            bundles.code("calendar").unwrap().to_vec(),
            None,
            Determinism::Enforced,
        )
        .unwrap()
        .code_hash;
        let created = Contracts::bare_instantiate(
            account(1),
            0,
            GAS,
            None,
            Code::Upload(bundles.code("directory").unwrap().to_vec()),
            input("new", (calendar, account(99), account(99), seed())),
            b"matching-v5".to_vec(),
            DebugInfo::Skip,
            CollectEvents::Skip,
        )
        .result
        .unwrap();
        assert!(!created.result.did_revert());
        created.account_id
    });
    (ext, directory)
}
fn projection(directory: &Account) -> Projection {
    let (roles, skills, metadata): Catalog = read(directory, "catalog");
    (
        roles,
        skills,
        metadata,
        read(directory, "workers"),
        read(directory, "skill_requests"),
    )
}
fn digest(directory: &Account) -> String {
    format!(
        "{:x}",
        H256::from(sha2_256(&projection(directory).encode()))
    )
}
fn id(seed: u8) -> EntityId {
    let mut bytes = [0; 16];
    StdRng::from_seed([seed; 32]).fill_bytes(&mut bytes);
    EntityId::from_bytes(bytes)
}
fn command_input(command: ProviderCommand, seed: u8, origin: AccountId32) -> Vec<u8> {
    let command = if matches!(command, ProviderCommand::RegisterWorker(_)) {
        let call = UnsignedContractCallV1::new(
            ProviderInstanceId::from_bytes([0; 16]),
            OperationId::from_bytes([0; 16]),
            origin,
            0,
            UnixSeconds::new(u64::MAX),
            command,
        )
        .unwrap();
        native_provider::creation::compact_creation(&call).unwrap()
    } else {
        command
    };
    match command {
        ProviderCommand::RegisterWorkerCore(request) => {
            input("register_worker_core", (request, id(seed)))
        }
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::SetScorePolicy(policy) => input("set_score_policy", policy),
        _ => panic!("incomplete M4 fixture command"),
    }
}
fn plan(milestones: Vec<(EntityId, MilestoneDefinition)>) -> impl Encode {
    milestones
        .into_iter()
        .map(|(id, definition)| {
            (
                id,
                definition.window,
                definition.coordinator_minutes,
                definition
                    .requirements
                    .into_iter()
                    .map(|r| (r.key, r.role_id, r.skill_ids, r.minutes))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>()
}
fn observed<T: Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => json!({"ok":value}),
        Err(error) => json!({"error":error}),
    }
}
fn apply(directory: &Account, action: Action) -> Value {
    match action {
        Action::Command {
            origin,
            command,
            seed,
        } => observed(decoded::<Effect>(call(
            &Account::new(origin.into_bytes()),
            directory,
            command_input(command, seed, origin),
        ))),
        Action::Contributions {
            project_id,
            author,
            votes,
        } => observed(decoded::<()>(call(
            &account(99),
            directory,
            input("apply_reputation", (project_id, author, votes)),
        ))),
        Action::Coordinator { client, week, seed } => {
            observed(decoded::<(AccountId32, EntityId)>(call(
                &account(99),
                directory,
                input("select_coordinator", (client, week, [seed; 32])),
            )))
        }
        Action::Assign {
            project_id,
            client,
            coordinator,
            milestones,
            seed,
        } => observed(decoded::<Vec<Vec<(u32, AccountId32)>>>(call(
            &account(99),
            directory,
            input(
                "assign_team",
                (
                    project_id,
                    client,
                    coordinator,
                    plan(milestones),
                    [seed; 32],
                ),
            ),
        ))),
    }
}
#[test]
fn canonical_reputation_matching_trace_preserves_every_result_and_reservation() {
    let fixture = fixture();
    assert_eq!(fixture["schemaVersion"], 1);
    let (mut ext, directory) = setup();
    for (index, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        ext.execute_with(|| {
            assert_eq!(
                apply(
                    &directory,
                    serde_json::from_value(step["action"].clone()).unwrap()
                ),
                step["outcome"],
                "step {index}"
            );
            assert_eq!(digest(&directory), step["stateDigest"], "step {index}");
            assert_eq!(
                json!(read::<ScorePolicy>(&directory, "score_policy")),
                step["policy"],
                "step {index}"
            );
        });
        if index % 30 == 29 {
            let bytes = Checkpoint::capture(ext).unwrap().to_bytes().unwrap();
            ext = Checkpoint::decode(&bytes).unwrap().restore().unwrap();
        }
    }
    ext.execute_with(|| assert_eq!(json!(projection(&directory)), fixture["final"]));
}
fn registration(directory: &Account, origin: u8) {
    let request = ProviderCommand::RegisterWorker(RegisterWorkerRequest {
        display_name: "Candidate".into(),
        qualifications: Qualifications {
            role_ids: vec![3],
            skill_ids: vec![1, 2],
        },
        calendar: CalendarDefinition {
            default_weekly_minutes: Minutes::new(600),
            overrides: vec![],
        },
    });
    assert!(
        decoded::<Effect>(call(
            &account(origin),
            directory,
            command_input(request, origin, AccountId32::from_bytes([origin; 32]))
        ))
        .is_ok()
    );
}
#[test]
fn project_hooks_require_controller_and_failed_vote_batch_rolls_back_all_changes() {
    let (mut ext, directory) = setup();
    ext.execute_with(|| {
        registration(&directory, 2);
        registration(&directory, 3);
        let before = digest(&directory);
        let votes = vec![(account(2), 0u8, 5u8, 37u32)];
        let vote_input = input("apply_reputation", ([90u8; 16], account(1), &votes));
        for caller in [1, 2, 3] {
            assert_eq!(
                decoded::<()>(call(&account(caller), &directory, vote_input.clone())).unwrap_err(),
                "reservation_controller_required"
            );
            let week = Week::new(2026, 1).unwrap();
            assert_eq!(
                decoded::<(AccountId32, EntityId)>(call(
                    &account(caller),
                    &directory,
                    input("select_coordinator", (account(1), week, [0u8; 32]))
                ))
                .unwrap_err(),
                "reservation_controller_required"
            );
            assert_eq!(
                decoded::<Vec<Vec<(u32, AccountId32)>>>(call(
                    &account(caller),
                    &directory,
                    input(
                        "assign_team",
                        (
                            [90u8; 16],
                            account(1),
                            account(2),
                            Vec::<u8>::new(),
                            [0u8; 32]
                        )
                    )
                ))
                .unwrap_err(),
                "reservation_controller_required"
            );
        }
        assert_eq!(digest(&directory), before);
        assert!(decoded::<()>(call(&account(99), &directory, vote_input.clone())).is_ok());
        let once = digest(&directory);
        let policy = ScorePolicy::new(Percentage::new(50).unwrap(), Percentage::new(50).unwrap())
            .unwrap()
            .with_coordinator_vote_weights(100, 0)
            .unwrap();
        assert!(
            decoded::<Effect>(call(
                &account(1),
                &directory,
                input("set_score_policy", policy)
            ))
            .is_ok()
        );
        assert!(decoded::<()>(call(&account(99), &directory, vote_input)).is_ok());
        assert_eq!(digest(&directory), once);
        let conflict = input(
            "apply_reputation",
            ([90u8; 16], account(1), vec![(account(2), 0u8, 1u8, 37u32)]),
        );
        assert_eq!(
            decoded::<()>(call(&account(99), &directory, conflict)).unwrap_err(),
            "evaluation_conflict"
        );
        assert_eq!(digest(&directory), once);
        for invalid in [
            vec![(account(2), 0u8, 5u8, 99u32), (account(3), 0, 0, 37)],
            vec![(account(2), 0u8, 5u8, 99u32), (account(101), 0, 5, 37)],
        ] {
            assert!(
                decoded::<()>(call(
                    &account(99),
                    &directory,
                    input("apply_reputation", ([91u8; 16], account(1), invalid))
                ))
                .is_err()
            );
            assert_eq!(digest(&directory), once);
        }
        // Invalid policy bytes fail ink decoding, before any domain mutation.
        let malformed = call(
            &account(1),
            &directory,
            input("set_score_policy", [50u8, 50, 0, 0]),
        );
        assert!(malformed.did_revert());
        assert_eq!(malformed.data, [1, 1]);
        assert_eq!(read::<ScorePolicy>(&directory, "score_policy"), policy);
    });
}

#[test]
fn competing_sqlite_workers_observe_committed_capacity_and_restart() {
    use contract_provider::worker::{
        Action as VmAction, Error, Invocation, ShellChanges, Status, Worker,
    };
    use std::{
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    async fn execute(
        client: &contract_provider::worker::Client,
        revision: i64,
        caller: u8,
        action: VmAction,
        data: Vec<u8>,
    ) -> Result<contract_provider::worker::Outcome, Error> {
        client
            .execute(
                revision,
                Invocation {
                    caller: account(caller),
                    action,
                    input: data,
                    gas: GAS,
                    timestamp_ms: 1_700_000_000_000,
                },
                ShellChanges {
                    success: vec![1],
                    rejection: vec![0],
                },
            )
            .unwrap()
            .result()
            .await
    }
    async fn start(path: &Path) -> Worker {
        Worker::start(
            path.to_path_buf(),
            vec![account(1), account(2), account(3), account(99)],
            vec![],
        )
        .await
        .unwrap()
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let path = std::env::temp_dir().join(format!(
                "ink-matching-{}-{}.sqlite",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let worker = start(&path).await;
            let client = worker.client();
            let definition = CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: vec![],
            };
            let uploaded = execute(
                &client,
                0,
                1,
                VmAction::Instantiate {
                    bundle: "calendar".into(),
                    salt: b"code-upload".to_vec(),
                },
                input("new", (account(1), account(99), [240u8; 16], &definition)),
            )
            .await
            .unwrap();
            assert_eq!(uploaded.status, Status::Success);
            let code = H256::from(blake2_256(
                Bundles::pinned().unwrap().code("calendar").unwrap(),
            ));
            let created = execute(
                &client,
                1,
                1,
                VmAction::Instantiate {
                    bundle: "directory".into(),
                    salt: b"matching-worker".to_vec(),
                },
                input("new", (code, account(99), account(99), seed())),
            )
            .await
            .unwrap();
            assert_eq!(created.status, Status::Success);
            let directory = created.address.unwrap();
            let request = protocol::provider_core::RegisterWorkerCoreRequest {
                content: protocol::provider_core::MetadataCommitmentV1 {
                    operation_id: OperationId::from_bytes([0; 16]),
                    hash: PayloadHash::from_bytes([9; 32]),
                },
                qualifications: Qualifications {
                    role_ids: vec![3],
                    skill_ids: vec![1, 2],
                },
                calendar: definition,
            };
            for (revision, origin) in [(2, 2), (3, 3)] {
                let outcome = execute(
                    &client,
                    revision,
                    origin,
                    VmAction::Call {
                        address: directory.clone(),
                    },
                    input("register_worker_core", (&request, id(origin))),
                )
                .await
                .unwrap();
                assert_eq!(outcome.status, Status::Success);
            }
            let second = start(&path).await;
            let other = second.client();
            let week = Week::new(2026, 1).unwrap();
            let milestones = vec![(
                [111u8; 16],
                WeekWindow::new(week, week).unwrap(),
                0u32,
                vec![(1u32, 2u32, vec![1u32, 2], 600u32)],
            )];
            let data = input(
                "assign_team",
                ([110u8; 16], account(1), account(2), milestones, [19u8; 32]),
            );
            // Both async tasks enqueue work on separate dedicated OS workers.
            let c1 = client.clone();
            let d1 = directory.clone();
            let data1 = data.clone();
            let first = tokio::spawn(async move {
                execute(&c1, 4, 99, VmAction::Call { address: d1 }, data1).await
            });
            let c2 = other.clone();
            let d2 = directory.clone();
            let data2 = data.clone();
            let second_call = tokio::spawn(async move {
                execute(&c2, 4, 99, VmAction::Call { address: d2 }, data2).await
            });
            let results = [first.await.unwrap(), second_call.await.unwrap()];
            assert_eq!(
                results
                    .iter()
                    .filter(|r| matches!(r,Ok(v) if v.status==Status::Success))
                    .count(),
                1
            );
            assert_eq!(
                results
                    .iter()
                    .filter(|r| matches!(r, Err(Error::StaleRevision)))
                    .count(),
                1
            );
            let before = client.diagnostics().unwrap().result().await.unwrap();
            let rejected = execute(
                &other,
                5,
                99,
                VmAction::Call {
                    address: directory.clone(),
                },
                data,
            )
            .await
            .unwrap();
            assert_eq!(rejected.status, Status::Reverted);
            assert_eq!(
                Result::<Result<Vec<Vec<(u32, AccountId32)>>, String>, u8>::decode_all(
                    &mut &rejected.data[..]
                )
                .unwrap()
                .unwrap()
                .unwrap_err(),
                "no_available_candidate"
            );
            assert_eq!(
                other.diagnostics().unwrap().result().await.unwrap().root,
                before.root
            );
            worker.shutdown().await.unwrap();
            second.shutdown().await.unwrap();
            let restarted = start(&path).await;
            let outcome = restarted
                .client()
                .query(Invocation {
                    caller: account(1),
                    action: VmAction::Call { address: directory },
                    input: input("worker", account(3)),
                    gas: GAS,
                    timestamp_ms: 1_700_000_000_000,
                })
                .unwrap()
                .result()
                .await
                .unwrap();
            let worker = Result::<Option<WorkerCore>, u8>::decode_all(&mut &outcome.data[..])
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(worker.calendar.reservations.len(), 1);
            assert_eq!(worker.calendar.reservations[0].minutes, Minutes::new(600));
            restarted.shutdown().await.unwrap();
            std::fs::remove_file(path).unwrap();
        });
}
