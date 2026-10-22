//! Native and WASM independently replay identical signed calls and captured inputs.
use frame_support::weights::Weight;
use generated_contracts::*;
use ink_v5_compatibility::{
    Account, GAS,
    worker::{Client, DirectoryFixture, Error, MarketplaceProjection, Worker},
};
use parity_scale_codec::{DecodeAll, Encode};
use provider_execution::{ExecutionInputs, Shell};
use serde_json::{Value, json};
use sp_core::{
    H256, Pair,
    hashing::{blake2_256, sha2_256},
    sr25519,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{
    future::Future,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: UnixSeconds = UnixSeconds::new(100);
fn trace() -> Value {
    serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/project-signed.json"
    ))
    .unwrap()
}
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn fixture(gas: Weight) -> DirectoryFixture {
    let initial: MarketplaceProjection =
        serde_json::from_value(trace()["initial"].clone()).unwrap();
    DirectoryFixture {
        root: account(1),
        funded_accounts: (2..=5).map(account).collect(),
        catalog: (initial.0.roles, initial.0.skills, initial.0.skill_roles),
        reservation_controller: account(4),
        reference_controller: account(4),
        generation: Some(ProviderInstanceId::from_bytes([107; 16])),
        gas_limit: gas,
        projects: true,
        money: None,
    }
}
fn signed(seed: u8, nonce: u64, op: u8, command: ProviderCommand) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        ProviderInstanceId::from_bytes([107; 16]),
        OperationId::from_bytes([op; 16]),
        account(seed),
        nonce,
        UnixSeconds::new(200),
        command,
    )
    .unwrap();
    resign(call, seed)
}
fn resign(call: UnsignedContractCallV1, seed: u8) -> SignedContractCallV1 {
    let signature = Sr25519Signature::from_bytes(
        sr25519::Pair::from_seed(&[seed; 32])
            .sign(&call.signable_bytes().unwrap())
            .0,
    );
    SignedContractCallV1 { call, signature }
}
fn inputs(seed: u8) -> ExecutionInputs {
    ExecutionInputs {
        version: 1,
        now: NOW,
        seed: [seed; 32],
    }
}
fn hex(bytes: [u8; 32]) -> String {
    format!("{:x}", H256::from(bytes))
}
fn digest(shell: &Shell) -> String {
    hex(sha2_256(&serde_json::to_vec(shell).unwrap()))
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
            "ink-projects-signed-{}-{}.sqlite",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }
    async fn start(&self) -> Worker {
        Worker::start_directory_fixture(self.0.clone(), fixture(GAS))
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
    async fn stored(&self) -> (i64, Vec<u8>, Vec<u8>, Vec<u8>) {
        let pool = self.pool().await;
        let row = sqlx::query_as("SELECT revision,runtime,shell,outcome FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        row
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_file(&self.0).unwrap();
        }
    }
}
async fn shell(client: &Client) -> Shell {
    client.shell().unwrap().result().await.unwrap()
}
async fn projection(client: &Client) -> MarketplaceProjection {
    client
        .marketplace_projection(account(1))
        .unwrap()
        .result()
        .await
        .unwrap()
}

#[test]
fn native_signed_trace_matches_receipts_events_inputs_nonces_and_domain_after_every_call() {
    test(async {
        let trace = trace();
        let db = Database::new();
        let mut worker = db.start().await;
        assert_eq!(json!(projection(&worker.client()).await), trace["initial"]);
        for (index, step) in trace["steps"].as_array().unwrap().iter().enumerate() {
            let call = serde_json::from_value(step["signed"].clone()).unwrap();
            let inputs = serde_json::from_value(step["inputs"].clone()).unwrap();
            let result = worker
                .client()
                .execute_directory_with_inputs(call, inputs)
                .unwrap()
                .result()
                .await;
            let observed = match result {
                Ok(receipt) => json!({"receipt":receipt}),
                Err(Error::Shell(error)) => json!({"error":error.code()}),
                Err(Error::Invariant(code)) => json!({"error":code}),
                other => panic!("step {index}: {other:?}"),
            };
            assert_eq!(observed, step["outcome"], "step {index}");
            assert_eq!(
                digest(&shell(&worker.client()).await),
                step["shellDigest"],
                "step {index}"
            );
            assert_eq!(
                hex(sha2_256(&projection(&worker.client()).await.encode())),
                step["stateDigest"],
                "step {index}"
            );
            if index == 9 || index == 26 || index == 40 || index == 62 {
                worker.shutdown().await.unwrap();
                worker = db.start().await;
            }
        }
        assert_eq!(json!(projection(&worker.client()).await), trace["final"]);
        let before = db.stored().await;
        // Authenticated replay ignores expiry and a different comparison seed.
        let step = &trace["steps"][1];
        let call = serde_json::from_value(step["signed"].clone()).unwrap();
        let receipt = worker
            .client()
            .execute_directory_with_inputs(
                call,
                ExecutionInputs {
                    now: UnixSeconds::new(10_001),
                    ..inputs(240)
                },
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(json!(receipt), step["outcome"]["receipt"]);
        assert_eq!(db.stored().await, before);
        worker.shutdown().await.unwrap();
    });
}

async fn execute_step(client: &Client, index: usize) -> Result<OperationReceipt, Error> {
    let step = &trace()["steps"][index];
    client
        .execute_directory_with_inputs(
            serde_json::from_value(step["signed"].clone()).unwrap(),
            serde_json::from_value(step["inputs"].clone()).unwrap(),
        )
        .unwrap()
        .result()
        .await
}
async fn prepare_factory(client: &Client) {
    for index in 0..4 {
        execute_step(client, index).await.unwrap();
    }
}
#[test]
fn factory_sql_failure_discards_child_registry_and_shell_then_competing_writers_replay_once() {
    test(async {
        let db = Database::new();
        let first = db.start().await;
        let client = first.client();
        prepare_factory(&client).await;
        let before = db.stored().await;
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER reject_commit BEFORE UPDATE ON candidate_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
        assert!(matches!(
            execute_step(&client, 4).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(db.stored().await, before);
        assert!(projection(&client).await.3.is_empty());
        sqlx::query("DROP TRIGGER reject_commit")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let second = db.start().await;
        let step = &trace()["steps"][4];
        let a = client
            .execute_directory_with_inputs(
                serde_json::from_value(step["signed"].clone()).unwrap(),
                serde_json::from_value(step["inputs"].clone()).unwrap(),
            )
            .unwrap();
        let b = second
            .client()
            .execute_directory_with_inputs(
                serde_json::from_value(step["signed"].clone()).unwrap(),
                serde_json::from_value(step["inputs"].clone()).unwrap(),
            )
            .unwrap();
        assert_eq!(a.result().await.unwrap(), b.result().await.unwrap());
        assert_eq!(db.stored().await.0, 5);
        assert_eq!(projection(&client).await.3.len(), 1);
        let before = db.stored().await;
        // A generated-ID collision is an infrastructure failure, not a nonce-consuming rejection.
        assert!(matches!(
            execute_step(&client, 5).await,
            Err(Error::Invariant("invalid_project_state"))
        ));
        assert_eq!(db.stored().await, before);
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
        let restarted = db.start().await;
        assert_eq!(db.stored().await, before);
        assert_eq!(projection(&restarted.client()).await.3.len(), 1);
        restarted.shutdown().await.unwrap();
    });
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = blake2_256(name.as_bytes())[..4].to_vec();
    data.extend(args.encode());
    data
}
#[test]
fn direct_permissions_and_query_isolation_preserve_project_registry_and_global_ids() {
    use ink_v5_compatibility::worker::{Action, Invocation, Status};
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        prepare_factory(&client).await;
        execute_step(&client, 4).await.unwrap();
        let before = db.stored().await;
        let persisted: Value = serde_json::from_slice(&before.2).unwrap();
        let directory: Account = serde_json::from_value(persisted["directory"].clone()).unwrap();
        let project_id = projection(&client).await.3[0].project_id;
        for (caller, data) in [
            // Fixed legacy fixture accounts lose controller access once actual
            // Project code is configured. Registered Projects retain the hooks.
            (4, input("next_task_key", ())),
            (
                4,
                input("set_proposal_references", (project_id, Option::<u8>::None)),
            ),
            (
                4,
                input(
                    "apply_reputation",
                    (project_id, account(4), vec![(account(2), 1u8, 5u8, 30u32)]),
                ),
            ),
            (
                4,
                input(
                    "assign_team",
                    (
                        project_id,
                        account(4),
                        account(2),
                        Vec::<u8>::new(),
                        [0u8; 32],
                    ),
                ),
            ),
            (2, input("next_task_key", ())),
            (
                2,
                input(
                    "set_project_identities",
                    Vec::<(EntityId, Vec<(EntityId, EntityId)>)>::new(),
                ),
            ),
            (2, input("configure_project_code", H256::zero())),
            // Even root cannot change the installed immutable code hash.
            (1, input("configure_project_code", H256::zero())),
        ] {
            let outcome = client
                .query(Invocation {
                    caller: Account::new(account(caller).into_bytes()),
                    action: Action::Call {
                        address: directory.clone(),
                    },
                    input: data,
                    gas: GAS,
                    timestamp_ms: 100_000,
                })
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(outcome.status, Status::Reverted);
            assert!(
                Result::<Result<(), String>, u8>::decode_all(&mut &outcome.data[..])
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
        }
        // An account cannot construct an unregistered Project by naming itself as Directory.
        let outcome = client
            .query(Invocation {
                caller: Account::new(account(2).into_bytes()),
                action: Action::Instantiate {
                    bundle: "projects-v5".into(),
                    salt: vec![42],
                },
                input: input(
                    "new",
                    (
                        account(2),
                        EntityId::from_bytes([42; 16]),
                        account(4),
                        account(2),
                        "Forged".to_string(),
                        "".to_string(),
                        100u64,
                    ),
                ),
                gas: GAS,
                timestamp_ms: 100_000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::Reverted);
        // An otherwise valid root mutation is rolled back by the query path.
        let outcome = client
            .query(Invocation {
                caller: Account::new(account(1).into_bytes()),
                action: Action::Call { address: directory },
                input: input("set_compensation_policy", 33u8),
                gas: GAS,
                timestamp_ms: 100_000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(outcome.status, Status::Success);
        assert_eq!(db.stored().await, before);
        assert_eq!(
            projection(&client)
                .await
                .0
                .compensation_policy
                .coordinator_percent,
            Percentage::new(10).unwrap()
        );
        // Unsupported execution cannot consume an operation, even with a valid signature.
        let call = signed(
            4,
            0,
            240,
            ProviderCommand::ApproveExecution {
                project_id,
                proposal_id: EntityId::from_bytes([1; 16]),
                expected_revision: 0,
            },
        );
        assert!(matches!(
            client.execute_signed(call, NOW).unwrap().result().await,
            Err(Error::IncompleteCoverage)
        ));
        assert_eq!(db.stored().await, before);
        // Reject changed coverage configuration on restart.
        worker.shutdown().await.unwrap();
        let mut wrong = fixture(GAS);
        wrong.projects = false;
        assert!(
            Worker::start_directory_fixture(db.0.clone(), wrong)
                .await
                .is_err()
        );
        let worker = db.start().await;
        assert_eq!(db.stored().await, before);
        worker.shutdown().await.unwrap();
    });
}
