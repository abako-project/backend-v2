//! Calendar behavior, permissions and persistence through Dao’s public WASM API.
use contract_provider::{
    Account, GAS, Runtime,
    worker::{Action, Client, Invocation, Outcome, ShellChanges, Status, Worker},
};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use protocol::provider_core::{
    CatalogEntryCore, MetadataCommitmentV1, RegisterWorkerCoreRequest, SkillRoleCore,
};
use protocol::*;
type Effect = (
    DomainEventKind,
    Option<EntityId>,
    Vec<AccountId32>,
    Option<u32>,
);
use serde_json::{Value, from_value, json};
use sp_core::hashing::blake2_256;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ink-calendar-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    async fn start(&self) -> Worker {
        Worker::start(self.0.clone(), (1..=4).map(account).collect(), vec![])
            .await
            .unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_file(&self.0).unwrap();
        }
    }
}
fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut input = blake2_256(name.as_bytes())[..4].to_vec();
    input.extend(args.encode());
    input
}
fn call(address: &Account, caller: u8, name: &str, args: impl Encode) -> Invocation {
    Invocation {
        caller: account(caller),
        action: Action::Call {
            address: address.clone(),
        },
        input: input(name, args),
        gas: GAS,
        timestamp_ms: 1_700_000_000_001,
    }
}
fn shell() -> ShellChanges {
    ShellChanges {
        success: vec![0],
        rejection: vec![1],
    }
}
fn decode<T: Decode>(outcome: &Outcome) -> Result<T, String> {
    Result::<Result<T, String>, u8>::decode_all(&mut &outcome.data[..])
        .unwrap()
        .unwrap()
}
async fn deploy(client: &Client, definition: &CalendarDefinition) -> Account {
    let outcome = client
        .execute(
            0,
            Invocation {
                caller: account(1),
                action: Action::Instantiate {
                    bundle: "dao".into(),
                    salt: vec![1],
                },
                input: input(
                    "new",
                    (
                        account(2),
                        account(2),
                        (
                            vec![CatalogEntryCore {
                                id: 1,
                                fixed: true,
                                content: protocol::provider_core::CatalogReferenceV1::Seed(
                                    PayloadHash::from_bytes([7; 32]),
                                ),
                            }],
                            Vec::<CatalogEntryCore>::new(),
                            Vec::<SkillRoleCore>::new(),
                        ),
                    ),
                ),
                gas: GAS,
                timestamp_ms: 1_700_000_000_001,
            },
            shell(),
        )
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(outcome.status, Status::Success, "{outcome:?}");
    let address = outcome.address.unwrap();
    register(client, &address, 3, [1; 16], definition).await;
    address
}
async fn register(
    client: &Client,
    address: &Account,
    owner: u8,
    id: [u8; 16],
    definition: &CalendarDefinition,
) {
    let revision = client
        .diagnostics()
        .unwrap()
        .result()
        .await
        .unwrap()
        .revision;
    let request = RegisterWorkerCoreRequest {
        content: MetadataCommitmentV1 {
            operation_id: OperationId::from_bytes([0; 16]),
            hash: PayloadHash::from_bytes([9; 32]),
        },
        qualifications: Qualifications {
            role_ids: vec![],
            skill_ids: vec![],
        },
        calendar: definition.clone(),
    };
    let result = client
        .execute(
            revision,
            call(address, owner, "register_worker", (request, id)),
            shell(),
        )
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert!(decode::<Effect>(&result).is_ok());
}
async fn view(client: &Client, address: &Account) -> CalendarView {
    let outcome = client
        .query(call(address, 4, "calendar", account(3)))
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(outcome.status, Status::Success);
    decode::<CalendarView>(&outcome).unwrap()
}
fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

#[test]
fn calendar_cases_and_restart() {
    run(async {
        let fixture: Value = serde_json::from_str(include_str!("fixtures/calendar.json")).unwrap();
        assert_eq!(fixture["schemaVersion"], 1);
        for case in fixture["cases"].as_array().unwrap() {
            let db = Database::new();
            let worker = db.start().await;
            let client = worker.client();
            let mut expected_view: CalendarView = from_value(case["initial"].clone()).unwrap();
            let address = deploy(&client, &expected_view.definition).await;
            assert_eq!(view(&client, &address).await, expected_view);
            let mut revision = 2;
            for action in case["actions"].as_array().unwrap() {
                let before = client.diagnostics().unwrap().result().await.unwrap();
                let mutating = matches!(action["kind"].as_str(), Some("reserve" | "setCalendar"));
                let invocation = match action["kind"].as_str().unwrap() {
                    "available" => call(
                        &address,
                        4,
                        "calendar_available",
                        (
                            account(3),
                            from_value::<Week>(action["week"].clone()).unwrap(),
                        ),
                    ),
                    "allocation" => call(
                        &address,
                        4,
                        "calendar_allocation",
                        (
                            account(3),
                            from_value::<WeekWindow>(action["window"].clone()).unwrap(),
                            from_value::<Minutes>(action["minutes"].clone()).unwrap(),
                        ),
                    ),
                    "reserve" => call(
                        &address,
                        2,
                        "reserve_calendar",
                        (
                            account(3),
                            from_value::<WeekWindow>(action["window"].clone()).unwrap(),
                            from_value::<Minutes>(action["minutes"].clone()).unwrap(),
                            ReservationView {
                                project_id: protocol::EntityId::from_bytes([10; 16]),
                                milestone_id: Some(protocol::EntityId::from_bytes([11; 16])),
                                requirement_key: Some(7),
                                week: Week::new(2026, 1).unwrap(),
                                minutes: Minutes::ZERO,
                            },
                        ),
                    ),
                    "setCalendar" => call(
                        &address,
                        3,
                        "set_calendar",
                        from_value::<CalendarDefinition>(action["definition"].clone()).unwrap(),
                    ),
                    _ => panic!("unknown calendar action"),
                };
                let outcome = if mutating {
                    let result = client
                        .execute(revision, invocation, shell())
                        .unwrap()
                        .result()
                        .await
                        .unwrap();
                    revision += 1;
                    result
                } else {
                    client.query(invocation).unwrap().result().await.unwrap()
                };
                let result = match action["kind"].as_str().unwrap() {
                    "available" => decode::<Minutes>(&outcome).map(|n| json!(n)),
                    "allocation" => {
                        decode::<Vec<(Week, Minutes)>>(&outcome).map(|slots| json!(slots))
                    }
                    "setCalendar" => decode::<Effect>(&outcome).map(|_| Value::Null),
                    _ => decode::<()>(&outcome).map(|()| Value::Null),
                };
                if let Some(error) = action["error"].as_str() {
                    assert_eq!(outcome.status, Status::Reverted, "{case}");
                    assert_eq!(result, Err(error.into()));
                    assert_eq!(
                        client.diagnostics().unwrap().result().await.unwrap().root,
                        before.root
                    );
                } else {
                    assert_eq!(outcome.status, Status::Success, "{outcome:?}");
                    if mutating {
                        result.unwrap();
                        expected_view.reservations =
                            from_value(action["expected"].clone()).unwrap();
                        if action["kind"] == "setCalendar" {
                            expected_view.definition =
                                from_value(action["definition"].clone()).unwrap();
                        }
                    } else {
                        assert_eq!(
                            result.unwrap(),
                            action["expected"],
                            "{}: {action}",
                            case["name"]
                        );
                    }
                }
                assert_eq!(
                    view(&client, &address).await,
                    expected_view,
                    "{}: {action}",
                    case["name"]
                );
            }
            worker.shutdown().await.unwrap();
            let worker = db.start().await;
            assert_eq!(view(&worker.client(), &address).await, expected_view);
            assert_eq!(
                worker
                    .client()
                    .diagnostics()
                    .unwrap()
                    .result()
                    .await
                    .unwrap()
                    .revision,
                revision
            );
            worker.shutdown().await.unwrap();
        }
    });
}

#[test]
fn caller_permissions_and_dry_runs_preserve_calendar() {
    run(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let definition = CalendarDefinition {
            default_weekly_minutes: Minutes::new(60),
            overrides: vec![],
        };
        let address = deploy(&client, &definition).await;
        let initial = view(&client, &address).await;
        let week = Week::new(2026, 1).unwrap();
        let window = WeekWindow::new(week, week).unwrap();
        let reference = ReservationView {
            project_id: protocol::EntityId::from_bytes([9; 16]),
            milestone_id: None,
            requirement_key: None,
            week,
            minutes: Minutes::ZERO,
        };
        let mut revision = 2;
        // Neither the calendar owner nor the operator may invent project reservations.
        for caller in [1, 3, 4] {
            let before = client.diagnostics().unwrap().result().await.unwrap();
            let outcome = client
                .execute(
                    revision,
                    call(
                        &address,
                        caller,
                        "reserve_calendar",
                        (account(3), window, Minutes::new(1), &reference),
                    ),
                    shell(),
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            revision += 1;
            assert_eq!(outcome.status, Status::Reverted);
            assert_eq!(
                decode::<()>(&outcome),
                Err("reservation_controller_required".into())
            );
            assert_eq!(
                client.diagnostics().unwrap().result().await.unwrap().root,
                before.root
            );
            assert_eq!(view(&client, &address).await, initial);
        }
        for caller in [2, 4] {
            let outcome = client
                .execute(
                    revision,
                    call(&address, caller, "set_calendar", &definition),
                    shell(),
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            revision += 1;
            assert_eq!(outcome.status, Status::Reverted);
            assert_eq!(decode::<()>(&outcome), Err("worker_not_found".into()));
            assert_eq!(view(&client, &address).await, initial);
        }
        let changed = CalendarDefinition {
            default_weekly_minutes: Minutes::new(7),
            overrides: vec![],
        };
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let outcome = client
            .query(call(&address, 3, "set_calendar", &changed))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert!(decode::<Effect>(&outcome).is_ok());
        assert_eq!(view(&client, &address).await, initial);
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!((after.root, after.revision), (before.root, before.revision));
        // A committed owner write updates only their own calendar.
        let outcome = client
            .execute(
                revision,
                call(&address, 3, "set_calendar", &changed),
                shell(),
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert!(decode::<Effect>(&outcome).is_ok());
        assert_eq!(view(&client, &address).await.definition, changed);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn malformed_week_ranges_and_gas_exhaustion_do_not_commit() {
    run(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let address = deploy(
            &client,
            &CalendarDefinition {
                default_weekly_minutes: Minutes::ZERO,
                overrides: vec![],
            },
        )
        .await;
        let before = client.diagnostics().unwrap().result().await.unwrap();
        // Encode raw tuples because validated public DTOs cannot represent these.
        for (index, week) in [(2026_i32, 0_u8), (2025, 53), (i32::MAX, 1)]
            .into_iter()
            .enumerate()
        {
            let outcome = client
                .execute(
                    2 + index as i64,
                    call(&address, 3, "calendar_available", (account(3), week)),
                    shell(),
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(outcome.status, Status::LanguageError, "{outcome:?}");
        }
        let outcome = client
            .query(call(
                &address,
                4,
                "calendar_allocation",
                (account(3), ((2027_i32, 2_u8), (2027_i32, 1_u8)), 0_u32),
            ))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::LanguageError, "{outcome:?}");
        let mut invocation = call(
            &address,
            4,
            "calendar_allocation",
            (
                account(3),
                WeekWindow::new(Week::new(1000, 1).unwrap(), Week::new(3000, 1).unwrap()).unwrap(),
                Minutes::new(1),
            ),
        );
        invocation.gas =
            frame_support::weights::Weight::from_parts(10_000_000_000, 16 * 1024 * 1024);
        let gas_limit = invocation.gas;
        let outcome = client
            .execute(5, invocation, shell())
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            outcome.status,
            Status::Dispatch(pallet_contracts::Error::<Runtime>::OutOfGas.into())
        );
        assert!(outcome.gas_consumed.all_lte(gas_limit));
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!(after.root, before.root);
        assert_eq!(after.revision, 6);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn competing_reservations_observe_committed_capacity() {
    run(async {
        let db = Database::new();
        let first = db.start().await;
        let second = db.start().await;
        let one = first.client();
        let two = second.client();
        let address = deploy(
            &one,
            &CalendarDefinition {
                default_weekly_minutes: Minutes::new(60),
                overrides: vec![],
            },
        )
        .await;
        let week = Week::new(2026, 1).unwrap();
        let reference = ReservationView {
            project_id: protocol::EntityId::from_bytes([9; 16]),
            milestone_id: None,
            requirement_key: None,
            week,
            minutes: Minutes::ZERO,
        };
        let reserve = || {
            call(
                &address,
                2,
                "reserve_calendar",
                (
                    account(3),
                    WeekWindow::new(week, week).unwrap(),
                    Minutes::new(40),
                    &reference,
                ),
            )
        };
        let a = one.execute(2, reserve(), shell()).unwrap();
        let b = two.execute(2, reserve(), shell()).unwrap();
        match (a.result().await, b.result().await) {
            (Ok(outcome), Err(contract_provider::worker::Error::StaleRevision))
            | (Err(contract_provider::worker::Error::StaleRevision), Ok(outcome)) => {
                assert_eq!(outcome.status, Status::Success);
                assert_eq!(decode::<()>(&outcome), Ok(()));
            }
            other => panic!("unexpected reservation race: {other:?}"),
        }
        let before = view(&one, &address).await;
        assert_eq!(before.reservations.len(), 1);
        let retry = two
            .execute(3, reserve(), shell())
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(retry.status, Status::Reverted);
        assert_eq!(decode::<()>(&retry), Err("insufficient_capacity".into()));
        assert_eq!(view(&two, &address).await, before);
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
        let restarted = db.start().await;
        assert_eq!(view(&restarted.client(), &address).await, before);
        restarted.shutdown().await.unwrap();
    });
}

#[test]
fn ordinary_reservation_growth_keeps_fixed_cells_and_reopens_after_failure() {
    run(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let address = deploy(
            &client,
            &CalendarDefinition {
                default_weekly_minutes: Minutes::new(1),
                overrides: Vec::new(),
            },
        )
        .await;
        let week = Week::new(2026, 1).unwrap();
        let record = ReservationView {
            project_id: protocol::EntityId::from_bytes([10; 16]),
            milestone_id: Some(protocol::EntityId::from_bytes([11; 16])),
            requirement_key: Some(7),
            week,
            minutes: Minutes::ZERO,
        };
        let before_limit = client.diagnostics().unwrap().result().await.unwrap();
        let oversized = client
            .execute(
                2,
                call(
                    &address,
                    2,
                    "reserve_calendar",
                    (
                        account(3),
                        WeekWindow::new(week, Week::new(2046, 1).unwrap()).unwrap(),
                        Minutes::new(1000),
                        &record,
                    ),
                ),
                shell(),
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            oversized.status,
            Status::Dispatch(pallet_contracts::Error::<Runtime>::OutOfGas.into()),
            "{oversized:?}"
        );
        assert_eq!(
            client.diagnostics().unwrap().result().await.unwrap().root,
            before_limit.root
        );
        let mut revision = 3;
        for year in 2026..2046 {
            let window =
                WeekWindow::new(Week::new(year, 1).unwrap(), Week::new(year, 50).unwrap()).unwrap();
            let outcome = client
                .execute(
                    revision,
                    call(
                        &address,
                        2,
                        "reserve_calendar",
                        (account(3), window, Minutes::new(50), &record),
                    ),
                    shell(),
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(outcome.status, Status::Success, "{outcome:?}");
            revision += 1;
        }
        let expected = view(&client, &address).await;
        assert_eq!(expected.reservations.len(), 1000);
        assert!(expected.encode().len() > 16 * 1024);
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let failed = client
            .execute(
                revision,
                call(
                    &address,
                    3,
                    "set_calendar",
                    CalendarDefinition {
                        default_weekly_minutes: Minutes::ZERO,
                        overrides: Vec::new(),
                    },
                ),
                shell(),
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(failed.status, Status::Reverted);
        assert_eq!(decode::<()>(&failed), Err("calendar_overcommitted".into()));
        assert_eq!(
            client.diagnostics().unwrap().result().await.unwrap().root,
            before.root
        );
        worker.shutdown().await.unwrap();
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.0.display()))
            .await
            .unwrap();
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT runtime FROM provider_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        let mut ext = contract_provider::checkpoint::Checkpoint::decode(&bytes)
            .unwrap()
            .restore()
            .unwrap();
        ext.execute_with(|| {
            let header = contract_provider::Contracts::get_storage(
                address.clone(),
                (0x4341_4c01u32, account(3)).encode(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(header.len(), 16);
            for index in [0u32, 999] {
                let value = contract_provider::Contracts::get_storage(
                    address.clone(),
                    (0x4341_4c04u32, (account(3), index)).encode(),
                )
                .unwrap()
                .unwrap();
                assert_eq!(value.len(), 47);
            }
            assert!(
                contract_provider::Contracts::get_storage(
                    address.clone(),
                    (u32::MAX, 0u32).encode()
                )
                .unwrap()
                .is_none()
            );
        });
        let reopened = db.start().await;
        assert_eq!(view(&reopened.client(), &address).await, expected);
        reopened.shutdown().await.unwrap();
    });
}

#[test]
fn workers_keep_independent_overrides_and_reservations_after_restart() {
    run(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let week = Week::new(2026, 1).unwrap();
        let definition = CalendarDefinition {
            default_weekly_minutes: Minutes::new(60),
            overrides: vec![WeekOverride {
                week,
                capacity: Minutes::new(100),
            }],
        };
        let address = deploy(&client, &definition).await;
        register(&client, &address, 4, [2; 16], &definition).await;
        let reference = ReservationView {
            project_id: EntityId::from_bytes([9; 16]),
            milestone_id: None,
            requirement_key: None,
            week,
            minutes: Minutes::ZERO,
        };
        let reserved = client
            .execute(
                3,
                call(
                    &address,
                    2,
                    "reserve_calendar",
                    (
                        account(3),
                        WeekWindow::new(week, week).unwrap(),
                        Minutes::new(40),
                        reference,
                    ),
                ),
                shell(),
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(decode::<()>(&reserved), Ok(()));
        let changed = CalendarDefinition {
            default_weekly_minutes: Minutes::new(60),
            overrides: vec![],
        };
        let edited = client
            .execute(4, call(&address, 3, "set_calendar", &changed), shell())
            .unwrap()
            .result()
            .await
            .unwrap();
        assert!(decode::<Effect>(&edited).is_ok());
        let first = view(&client, &address).await;
        assert_eq!(first.definition, changed);
        assert_eq!(first.reservations.len(), 1);
        let second = decode::<CalendarView>(
            &client
                .query(call(&address, 1, "calendar", account(4)))
                .unwrap()
                .result()
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(second.calendar_id, EntityId::from_bytes([2; 16]));
        assert_eq!(second.definition, definition);
        assert!(second.reservations.is_empty());
        for (owner, expected) in [(3, 20), (4, 100)] {
            let available = client
                .query(call(
                    &address,
                    1,
                    "calendar_available",
                    (account(owner), week),
                ))
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(decode::<Minutes>(&available), Ok(Minutes::new(expected)));
        }
        worker.shutdown().await.unwrap();
        let worker = db.start().await;
        assert_eq!(view(&worker.client(), &address).await, first);
        let restored = worker
            .client()
            .query(call(&address, 1, "calendar", account(4)))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(decode::<CalendarView>(&restored).unwrap(), second);
        worker.shutdown().await.unwrap();
    });
}
