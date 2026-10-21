//! The same transport shell used by the live Rust provider, with real v5 WASM.
use frame_support::weights::Weight;
use generated_contracts::{
    AccountId32, CalendarDefinition, EntityId, ExecutionOutcome, Minutes, OperationId,
    ProviderCommand, ProviderInstanceId, SignedContractCallV1, Sr25519Signature, UnixSeconds,
    UnsignedContractCallV1,
};
use ink_v5_compatibility::{
    Account, GAS,
    worker::{Action, CalendarFixture, Client, Error, FixtureWorker, Invocation, Status, Worker},
};
use parity_scale_codec::{DecodeAll, Encode};
use sp_core::{Pair, hashing::blake2_256, sr25519};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{
    future::Future,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: UnixSeconds = UnixSeconds::new(1_700_000_001);
fn key(seed: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[seed; 32])
}
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(key(seed).public().0)
}
fn calendar(minutes: u32) -> ProviderCommand {
    ProviderCommand::SetCalendar(CalendarDefinition {
        default_weekly_minutes: Minutes::new(minutes),
        overrides: vec![],
    })
}
fn fixture(gas: Weight) -> CalendarFixture {
    CalendarFixture {
        root: account(1),
        funded_accounts: vec![account(2), account(3)],
        workers: vec![FixtureWorker {
            account: account(2),
            calendar_id: EntityId::from_bytes([42; 16]),
        }],
        gas_limit: gas,
    }
}
fn signed(
    info: ProviderInstanceId,
    seed: u8,
    nonce: u64,
    op: u8,
    command: ProviderCommand,
    expires: UnixSeconds,
) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        info,
        OperationId::from_bytes([op; 16]),
        account(seed),
        nonce,
        expires,
        command,
    )
    .unwrap();
    let signature = Sr25519Signature::from_bytes(key(seed).sign(&call.signable_bytes().unwrap()).0);
    SignedContractCallV1 { call, signature }
}
fn test(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ink-signed-{}-{}.sqlite",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }
    async fn start(&self) -> Worker {
        Worker::start_calendar_fixture(self.0.clone(), fixture(GAS))
            .await
            .unwrap()
    }
    async fn pool(&self) -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&self.0))
            .await
            .unwrap()
    }
    async fn availability(&self, client: &Client, seed: u8) -> u8 {
        let pool = self.pool().await;
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT shell FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let address: Account = serde_json::from_value(state["calendar"].clone()).unwrap();
        pool.close().await;
        let mut input = blake2_256(b"get_availability_hours")[..4].to_vec();
        input.extend(Account::new(account(seed).into_bytes()).encode());
        let outcome = client
            .query(Invocation {
                caller: Account::new(account(seed).into_bytes()),
                action: Action::Call { address },
                input,
                gas: GAS,
                timestamp_ms: NOW.get() * 1000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::Success);
        Result::<u8, u8>::decode_all(&mut &outcome.data[..])
            .unwrap()
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
async fn info(client: &Client) -> ProviderInstanceId {
    client
        .shell()
        .unwrap()
        .result()
        .await
        .unwrap()
        .info
        .provider_instance_id
}
fn expires() -> UnixSeconds {
    UnixSeconds::new(NOW.get() + 60)
}
fn expect_shell_error(result: Result<generated_contracts::OperationReceipt, Error>, code: &str) {
    match result {
        Err(Error::Shell(error)) => assert_eq!(error.code(), code),
        other => panic!("expected {code}: {other:?}"),
    }
}

#[test]
fn signed_success_replay_and_restart_preserve_actual_receipt_inputs_and_child_storage() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let generation = info(&client).await;
        let call = signed(generation, 2, 0, 1, calendar(600), expires());
        let receipt = client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt.outcome, ExecutionOutcome::Success);
        assert_eq!(
            receipt.created_entity_id,
            Some(EntityId::from_bytes([42; 16]))
        );
        assert_eq!(
            (receipt.first_event_cursor, receipt.last_event_cursor),
            (Some(1), Some(1))
        );
        assert_eq!(db.availability(&client, 2).await, 10);
        assert_eq!(db.availability(&client, 1).await, 0);
        let before = client.shell().unwrap().result().await.unwrap();
        let inputs = before.receipts[&receipt.operation_id]
            .inputs
            .as_ref()
            .unwrap()
            .clone();
        assert_eq!(inputs.now, NOW);
        let replay = client
            .execute_signed(call.clone(), UnixSeconds::new(NOW.get() + 999))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(replay, receipt);
        assert_eq!(
            client
                .diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap()
                .revision,
            1
        );
        worker.shutdown().await.unwrap();
        let restarted = db.start().await;
        let client = restarted.client();
        assert_eq!(info(&client).await, generation);
        assert_eq!(db.availability(&client, 2).await, 10);
        let state = client.shell().unwrap().result().await.unwrap();
        assert_eq!(state.nonce(account(2)), 1);
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].origin, Some(account(2)));
        assert_eq!(
            state.receipts[&receipt.operation_id]
                .inputs
                .as_ref()
                .unwrap()
                .seed,
            inputs.seed
        );
        assert_eq!(
            client
                .execute_signed(call, UnixSeconds::new(NOW.get() + 1000))
                .unwrap()
                .result()
                .await
                .unwrap(),
            receipt
        );
        restarted.shutdown().await.unwrap();
    });
}
#[test]
fn invalid_envelopes_and_incomplete_coverage_never_consume_nonce_or_commit() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let generation = info(&client).await;
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let mut forged = signed(generation, 2, 0, 1, calendar(600), expires());
        forged.signature = Sr25519Signature::from_bytes([0; 64]);
        expect_shell_error(
            client.execute_signed(forged, NOW).unwrap().result().await,
            "invalid_signature",
        );
        for (nonce, expires, instance, reason) in [
            (
                0,
                UnixSeconds::new(NOW.get() - 1),
                generation,
                "expired_call",
            ),
            (1, expires(), generation, "nonce_conflict"),
            (
                0,
                expires(),
                ProviderInstanceId::from_bytes([9; 16]),
                "provider_instance_mismatch",
            ),
        ] {
            expect_shell_error(
                client
                    .execute_signed(signed(instance, 2, nonce, 1, calendar(600), expires), NOW)
                    .unwrap()
                    .result()
                    .await,
                reason,
            );
        }
        assert!(matches!(
            client
                .execute_signed(signed(generation, 2, 0, 1, calendar(61), expires()), NOW)
                .unwrap()
                .result()
                .await,
            Err(Error::IncompleteCoverage)
        ));
        let unsupported =
            ProviderCommand::CreateProject(generated_contracts::CreateProjectRequest {
                title: "Missing domain".into(),
                description: String::new(),
            });
        assert!(matches!(
            client
                .execute_signed(signed(generation, 2, 0, 1, unsupported, expires()), NOW)
                .unwrap()
                .result()
                .await,
            Err(Error::IncompleteCoverage)
        ));
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!((before.root, before.revision), (after.root, after.revision));
        let state = client.shell().unwrap().result().await.unwrap();
        assert_eq!(state.nonce(account(2)), 0);
        assert!(state.receipts.is_empty());
        assert!(state.events.is_empty());
        assert_eq!(db.availability(&client, 2).await, 0);
        worker.shutdown().await.unwrap();
    });
}
#[test]
fn domain_and_gas_rejections_record_failed_receipts_without_business_effects() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let generation = info(&client).await;
        let root = client.diagnostics().unwrap().result().await.unwrap().root;
        let call = signed(generation, 3, 0, 1, calendar(600), expires());
        let receipt = client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            receipt.outcome,
            ExecutionOutcome::Failed("worker_not_found".into())
        );
        assert_eq!(receipt.first_event_cursor, None);
        assert_eq!(
            client.diagnostics().unwrap().result().await.unwrap().root,
            root
        );
        let state = client.shell().unwrap().result().await.unwrap();
        assert_eq!(state.nonce(account(3)), 1);
        assert!(state.events.is_empty());
        assert_eq!(
            client
                .execute_signed(call, UnixSeconds::new(NOW.get() + 100))
                .unwrap()
                .result()
                .await
                .unwrap(),
            receipt
        );
        worker.shutdown().await.unwrap();
        let gas_worker = Worker::start_calendar_fixture(db.0.clone(), fixture(Weight::zero()))
            .await
            .unwrap();
        let client = gas_worker.client();
        let call = signed(generation, 2, 0, 2, calendar(600), expires());
        let receipt = client
            .execute_signed(call, NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            receipt.outcome,
            ExecutionOutcome::Failed("sandbox_out_of_gas".into())
        );
        assert_eq!(
            client.diagnostics().unwrap().result().await.unwrap().root,
            root
        );
        assert_eq!(
            client
                .shell()
                .unwrap()
                .result()
                .await
                .unwrap()
                .nonce(account(2)),
            1
        );
        gas_worker.shutdown().await.unwrap();
    });
}
#[test]
fn storage_failure_rolls_back_signed_runtime_nonce_receipt_and_events() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let generation = info(&client).await;
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER fail_commit AFTER UPDATE ON candidate_state BEGIN SELECT RAISE(ABORT, 'injected failure'); END").execute(&pool).await.unwrap();
        let call = signed(generation, 2, 0, 1, calendar(600), expires());
        assert!(matches!(
            client
                .execute_signed(call.clone(), NOW)
                .unwrap()
                .result()
                .await,
            Err(Error::Storage(_))
        ));
        let state = client.shell().unwrap().result().await.unwrap();
        assert_eq!(state.nonce(account(2)), 0);
        assert!(state.receipts.is_empty());
        assert!(state.events.is_empty());
        assert_eq!(db.availability(&client, 2).await, 0);
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!((before.root, before.revision), (after.root, after.revision));
        sqlx::query("DROP TRIGGER fail_commit")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let receipt = client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt.outcome, ExecutionOutcome::Success);
        worker.shutdown().await.unwrap();
        let restarted = db.start().await;
        assert_eq!(
            restarted
                .client()
                .execute_signed(call, UnixSeconds::new(NOW.get() + 100))
                .unwrap()
                .result()
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(db.availability(&restarted.client(), 2).await, 10);
        restarted.shutdown().await.unwrap();
    });
}
#[test]
fn competing_workers_replay_once_and_reject_same_nonce_or_conflicting_id() {
    test(async {
        let db = Database::new();
        let first = db.start().await;
        let second = db.start().await;
        let a = first.client();
        let b = second.client();
        let generation = info(&a).await;
        let call = signed(generation, 2, 0, 1, calendar(600), expires());
        let one = a.execute_signed(call.clone(), NOW).unwrap();
        let two = b.execute_signed(call, NOW).unwrap();
        assert_eq!(one.result().await.unwrap(), two.result().await.unwrap());
        assert_eq!(a.diagnostics().unwrap().result().await.unwrap().revision, 1);
        assert_eq!(b.shell().unwrap().result().await.unwrap().events.len(), 1);
        expect_shell_error(
            a.execute_signed(signed(generation, 2, 0, 1, calendar(1200), expires()), NOW)
                .unwrap()
                .result()
                .await,
            "idempotency_conflict",
        );
        let one = a
            .execute_signed(signed(generation, 2, 1, 2, calendar(1200), expires()), NOW)
            .unwrap();
        let two = b
            .execute_signed(signed(generation, 2, 1, 3, calendar(1800), expires()), NOW)
            .unwrap();
        match (one.result().await, two.result().await) {
            (Ok(_), Err(Error::Shell(error))) | (Err(Error::Shell(error)), Ok(_)) => {
                assert_eq!(error.code(), "nonce_conflict")
            }
            other => panic!("unexpected race result: {other:?}"),
        }
        assert_eq!(
            b.shell().unwrap().result().await.unwrap().nonce(account(2)),
            2
        );
        assert_eq!(b.diagnostics().unwrap().result().await.unwrap().revision, 2);
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
    });
}
#[test]
fn signed_store_rejects_raw_mutations_wrong_mode_and_root_without_reseeding() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let generation = info(&client).await;
        let invocation = Invocation {
            caller: Account::new(account(2).into_bytes()),
            action: Action::Call {
                address: Account::new([7; 32]),
            },
            input: vec![],
            gas: GAS,
            timestamp_ms: NOW.get() * 1000,
        };
        assert!(matches!(
            client.execute(
                0,
                invocation,
                ink_v5_compatibility::worker::ShellChanges {
                    success: vec![1],
                    rejection: vec![2]
                }
            ),
            Err(Error::InvalidState)
        ));
        worker.shutdown().await.unwrap();
        assert!(
            Worker::start(
                db.0.clone(),
                vec![Account::new(account(1).into_bytes())],
                vec![0]
            )
            .await
            .is_err()
        );
        let mut wrong = fixture(GAS);
        wrong.root = account(3);
        assert!(
            Worker::start_calendar_fixture(db.0.clone(), wrong)
                .await
                .is_err()
        );
        let restarted = db.start().await;
        let client = restarted.client();
        assert_eq!(info(&client).await, generation);
        assert!(
            client
                .shell()
                .unwrap()
                .result()
                .await
                .unwrap()
                .receipts
                .is_empty()
        );
        restarted.shutdown().await.unwrap();
    });
}
