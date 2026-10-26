use frame_support::weights::Weight;
use ink_sandbox::{
    Account, GAS, Runtime,
    worker::{Action, Client, Error, Invocation, MAX_INPUT, ShellChanges, Status, Worker},
};
use parity_scale_codec::{DecodeAll, Encode};
use sp_core::hashing::blake2_256;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{
    future::Future,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
fn invocation(action: Action, name: &str, args: impl Encode) -> Invocation {
    Invocation {
        caller: account(1),
        action,
        input: input(name, args),
        gas: GAS,
        timestamp_ms: 1_700_000_000_001,
    }
}
fn call(address: &Account, name: &str, args: impl Encode) -> Invocation {
    invocation(
        Action::Call {
            address: address.clone(),
        },
        name,
        args,
    )
}
fn shell(revision: u8) -> ShellChanges {
    ShellChanges {
        success: vec![revision, 0],
        rejection: vec![revision, 1],
    }
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
            "ink-worker-{}-{}.sqlite",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }
    async fn pool(&self) -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&self.0))
            .await
            .unwrap()
    }
    async fn start(&self) -> Worker {
        Worker::start(self.0.clone(), vec![account(1), account(2)], vec![0])
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
async fn deploy(
    client: &Client,
    revision: i64,
    bundle: &str,
    salt: u8,
    args: impl Encode,
) -> Account {
    let outcome = client
        .execute(
            revision,
            invocation(
                Action::Instantiate {
                    bundle: bundle.into(),
                    salt: vec![salt],
                },
                "new",
                args,
            ),
            shell(1),
        )
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(outcome.status, Status::Success);
    outcome.address.unwrap()
}
async fn value(client: &Client, probe: &Account) -> u32 {
    let outcome = client
        .query(call(probe, "value", ()))
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(outcome.status, Status::Success);
    Result::<u32, u8>::decode_all(&mut &outcome.data[..])
        .unwrap()
        .unwrap()
}
#[test]
fn durable_child_storage_and_mutating_queries_are_discarded() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let probe = deploy(&client, 0, "sandbox-probe", 1, ()).await;
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let dry_run = client
            .query(call(&probe, "set_value", 2_u32))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(dry_run.status, Status::Success);
        assert_eq!(value(&client, &probe).await, 0);
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!((before.root, before.revision), (after.root, after.revision));
        let outcome = client
            .execute(1, call(&probe, "set_value", 2_u32), shell(2))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::Success);
        worker.shutdown().await.unwrap();
        assert!(matches!(client.diagnostics(), Err(Error::Stopped)));
        let restarted = db.start().await;
        let client = restarted.client();
        assert_eq!(value(&client, &probe).await, 2);
        assert_eq!(
            client
                .diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap()
                .revision,
            2
        );
        let pool = db.pool().await;
        let row = sqlx::query("SELECT shell, outcome FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<Vec<u8>, _>("shell"), vec![2, 0]);
        assert_eq!(row.get::<Vec<u8>, _>("outcome"), outcome.encode());
        pool.close().await;
        restarted.shutdown().await.unwrap();
    });
}
#[test]
fn rejection_trap_language_error_and_gas_commit_only_failed_shell() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let parent = deploy(&client, 0, "sandbox-probe", 1, ()).await;
        let child = deploy(&client, 1, "sandbox-probe", 2, ()).await;
        let root = client.diagnostics().unwrap().result().await.unwrap().root;
        let cases = [
            (
                call(&parent, "write_then_revert", ()),
                Status::Reverted,
                vec![0, 1, 0],
            ),
            (
                call(&parent, "nested_failure", child.clone()),
                Status::Reverted,
                vec![0, 1, 0],
            ),
            (
                call(&parent, "write_then_trap", ()),
                Status::Dispatch(pallet_contracts::Error::<Runtime>::ContractTrapped.into()),
                vec![],
            ),
            (
                call(&parent, "missing_selector", ()),
                Status::LanguageError,
                vec![1, 1],
            ),
            (
                {
                    let mut call = call(&parent, "exhaust_gas", ());
                    call.gas = Weight::from_parts(10_000_000_000, 16 * 1024 * 1024);
                    call
                },
                Status::Dispatch(pallet_contracts::Error::<Runtime>::OutOfGas.into()),
                vec![],
            ),
        ];
        for (i, (invocation, status, data)) in cases.into_iter().enumerate() {
            let gas = invocation.gas;
            let outcome = client
                .execute(2 + i as i64, invocation, shell(3 + i as u8))
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!((outcome.status, outcome.data), (status, data));
            assert!(outcome.gas_consumed.all_lte(gas));
            let diagnostics = client.diagnostics().unwrap().result().await.unwrap();
            assert_eq!(diagnostics.root, root);
            assert_eq!(diagnostics.revision, 3 + i as i64);
        }
        for address in [parent, child] {
            let value = client
                .query(call(&address, "value", ()))
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(value.data, (0_u8, 0_u32).encode());
        }
        worker.shutdown().await.unwrap();
        let pool = db.pool().await;
        let shell: Vec<u8> = sqlx::query_scalar("SELECT shell FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(shell, vec![7, 1]);
        pool.close().await;
    });
}
#[test]
fn database_failure_rolls_back_runtime_shell_and_revision() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let probe = deploy(&client, 0, "sandbox-probe", 1, ()).await;
        let before = client.diagnostics().unwrap().result().await.unwrap();
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER fail_commit AFTER UPDATE ON candidate_state BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END").execute(&pool).await.unwrap();
        assert!(matches!(
            client
                .execute(1, call(&probe, "set_value", 2_u32), shell(2))
                .unwrap()
                .result()
                .await,
            Err(Error::Storage(_))
        ));
        assert_eq!(value(&client, &probe).await, 0);
        let after = client.diagnostics().unwrap().result().await.unwrap();
        assert_eq!((before.root, before.revision), (after.root, after.revision));
        let persisted: Vec<u8> = sqlx::query_scalar("SELECT shell FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(persisted, vec![1, 0]);
        sqlx::query("DROP TRIGGER fail_commit")
            .execute(&pool)
            .await
            .unwrap();
        let outcome = client
            .execute(1, call(&probe, "set_value", 2_u32), shell(2))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::Success);
        pool.close().await;
        worker.shutdown().await.unwrap();
        let worker = db.start().await;
        assert_eq!(value(&worker.client(), &probe).await, 2);
        worker.shutdown().await.unwrap();
    });
}
#[test]
fn competing_workers_use_latest_revision_and_corruption_never_reseeds() {
    test(async {
        let db = Database::new();
        let first = db.start().await;
        let second = db.start().await;
        let first_client = first.client();
        let second_client = second.client();
        let one = first_client
            .execute(
                0,
                invocation(
                    Action::Instantiate {
                        bundle: "sandbox-probe".into(),
                        salt: vec![1],
                    },
                    "new",
                    (),
                ),
                shell(1),
            )
            .unwrap();
        let two = second_client
            .execute(
                0,
                invocation(
                    Action::Instantiate {
                        bundle: "sandbox-probe".into(),
                        salt: vec![2],
                    },
                    "new",
                    (),
                ),
                shell(2),
            )
            .unwrap();
        let one = one.result().await;
        let two = two.result().await;
        let probe = match (one, two) {
            (Ok(one), Err(Error::StaleRevision)) => one.address.unwrap(),
            (Err(Error::StaleRevision), Ok(two)) => two.address.unwrap(),
            other => panic!("unexpected race result: {other:?}"),
        };
        assert_eq!(
            first_client
                .diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            second_client
                .diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            first_client
                .execute(1, call(&probe, "set_value", 2_u32), shell(3))
                .unwrap()
                .result()
                .await
                .unwrap()
                .status,
            Status::Success
        );
        assert_eq!(value(&second_client, &probe).await, 2);
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
        let pool = db.pool().await;
        sqlx::query("UPDATE candidate_state SET runtime = X'010203'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            Worker::start(db.0.clone(), vec![account(1)], vec![99])
                .await
                .is_err()
        );
        let row = sqlx::query("SELECT runtime, revision, shell FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<Vec<u8>, _>("runtime"), vec![1, 2, 3]);
        assert_eq!(row.get::<i64, _>("revision"), 2);
        assert_eq!(row.get::<Vec<u8>, _>("shell"), vec![3, 0]);
        pool.close().await;
    });
}
#[test]
fn resource_limits_and_unknown_bundle_do_not_commit() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let mut oversized = call(&account(2), "unused", ());
        oversized.input = vec![0; MAX_INPUT + 1];
        assert!(matches!(client.query(oversized), Err(Error::Limit)));
        let mut excessive = call(&account(2), "unused", ());
        excessive.gas = GAS.saturating_add(Weight::from_parts(1, 0));
        assert!(matches!(client.query(excessive), Err(Error::Limit)));
        let unknown = invocation(
            Action::Instantiate {
                bundle: "unknown".into(),
                salt: vec![],
            },
            "new",
            (),
        );
        assert!(matches!(
            client.execute(0, unknown, shell(1)).unwrap().result().await,
            Err(Error::Runtime(_))
        ));
        assert_eq!(
            client
                .diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap()
                .revision,
            0
        );
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn cancelled_queued_request_does_not_execute() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let pool = db.pool().await;
        let lock = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let first = client
            .execute(
                0,
                invocation(
                    Action::Instantiate {
                        bundle: "sandbox-probe".into(),
                        salt: vec![1],
                    },
                    "new",
                    (),
                ),
                shell(1),
            )
            .unwrap();
        let cancelled = client
            .execute(
                1,
                invocation(
                    Action::Instantiate {
                        bundle: "sandbox-probe".into(),
                        salt: vec![2],
                    },
                    "new",
                    (),
                ),
                shell(2),
            )
            .unwrap();
        drop(cancelled);
        lock.rollback().await.unwrap();
        assert_eq!(first.result().await.unwrap().status, Status::Success);
        // Diagnostics comes after the cancelled request in this worker's FIFO.
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
        pool.close().await;
        worker.shutdown().await.unwrap();
    });
}
