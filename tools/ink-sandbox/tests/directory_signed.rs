//! Native and WASM independently replay identical signed calls and captured inputs.
use frame_support::weights::Weight;
use generated_contracts::*;
use ink_v5_compatibility::{
    Account, GAS,
    worker::{Client, DirectoryFixture, DirectoryProjection, Error, Worker},
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
        "../../../services/mock-provider/src/domain/fixtures/directory-signed.json"
    ))
    .unwrap()
}
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn fixture(gas: Weight) -> DirectoryFixture {
    let initial: DirectoryProjection = serde_json::from_value(trace()["initial"].clone()).unwrap();
    DirectoryFixture {
        root: account(1),
        funded_accounts: (2..=4).map(account).collect(),
        catalog: (initial.0, initial.1, initial.2),
        reservation_controller: account(4),
        reference_controller: account(4),
        generation: Some(ProviderInstanceId::from_bytes([97; 16])),
        gas_limit: gas,
        projects: false,
    }
}
fn signed(seed: u8, nonce: u64, op: u8, command: ProviderCommand) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        ProviderInstanceId::from_bytes([97; 16]),
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
fn register() -> ProviderCommand {
    ProviderCommand::RegisterWorker(RegisterWorkerRequest {
        display_name: "Candidate".into(),
        qualifications: Qualifications {
            role_ids: vec![2],
            skill_ids: vec![1],
        },
        calendar: CalendarDefinition {
            default_weekly_minutes: Minutes::new(600),
            overrides: vec![],
        },
    })
}
fn calendar(minutes: u32) -> ProviderCommand {
    ProviderCommand::SetCalendar(CalendarDefinition {
        default_weekly_minutes: Minutes::new(minutes),
        overrides: vec![],
    })
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
            "ink-directory-signed-{}-{}.sqlite",
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
async fn projection(client: &Client) -> DirectoryProjection {
    client
        .directory_projection(account(1))
        .unwrap()
        .result()
        .await
        .unwrap()
}
fn shell_error(result: Result<OperationReceipt, Error>, expected: &str) {
    match result {
        Err(Error::Shell(error)) => assert_eq!(error.code(), expected),
        other => panic!("expected {expected}, got {other:?}"),
    }
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
            if index == 9 || index == 26 {
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
                    now: UnixSeconds::new(999),
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

#[test]
fn captured_ids_replay_read_isolation_and_request_visibility() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let call = signed(2, 0, 1, register());
        let receipt = client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        let state = shell(&client).await;
        let captured = state.receipts[&call.call.operation_id]
            .inputs
            .clone()
            .unwrap();
        assert_eq!(captured.now, NOW);
        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_seed(captured.seed);
        let mut expected = [0; 16];
        rand::Rng::fill_bytes(&mut rng, &mut expected);
        assert_eq!(
            receipt.created_entity_id,
            Some(EntityId::from_bytes(expected))
        );
        let registered = projection(&client).await;
        assert_eq!(
            registered.3[0].calendar.calendar_id,
            EntityId::from_bytes(expected)
        );
        let request = ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "New skill".into(),
            role_ids: vec![2],
        });
        client
            .execute_signed(signed(2, 1, 2, request), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(projection(&client).await.4.len(), 1);
        assert!(
            client
                .directory_projection(account(3))
                .unwrap()
                .result()
                .await
                .unwrap()
                .4
                .is_empty()
        );
        assert_eq!(
            client
                .directory_projection(account(2))
                .unwrap()
                .result()
                .await
                .unwrap()
                .4
                .len(),
            1
        );
        let before = db.stored().await;
        // Even a mutating query at the correct Directory address is discarded.
        let persisted: Value = serde_json::from_slice(&before.2).unwrap();
        let address: Account = serde_json::from_value(persisted["directory"].clone()).unwrap();
        let mut data = blake2_256(b"set_calendar")[..4].to_vec();
        let ProviderCommand::SetCalendar(definition) = calendar(37) else {
            unreachable!()
        };
        data.extend(definition.encode());
        client
            .query(ink_v5_compatibility::worker::Invocation {
                caller: Account::new(account(2).into_bytes()),
                action: ink_v5_compatibility::worker::Action::Call { address },
                input: data,
                gas: GAS,
                timestamp_ms: 100_000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(db.stored().await, before);
        assert_eq!(projection(&client).await.3, registered.3);
        worker.shutdown().await.unwrap();
        let worker = db.start().await;
        assert_eq!(
            worker
                .client()
                .execute_signed(call, UnixSeconds::new(999))
                .unwrap()
                .result()
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(db.stored().await, before);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn envelope_and_incomplete_coverage_failures_do_not_write_or_consume_nonces() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let before = db.stored().await;
        let call = signed(2, 0, 1, register());
        let mut bad = call.clone();
        bad.signature = Sr25519Signature::from_bytes([0; 64]);
        shell_error(
            client.execute_signed(bad, NOW).unwrap().result().await,
            "invalid_signature",
        );
        shell_error(
            client
                .execute_signed(call.clone(), UnixSeconds::new(200))
                .unwrap()
                .result()
                .await,
            "expired_call",
        );
        let mut wrong = call.call.clone();
        wrong.provider_instance_id = ProviderInstanceId::from_bytes([98; 16]);
        shell_error(
            client
                .execute_signed(resign(wrong, 2), NOW)
                .unwrap()
                .result()
                .await,
            "provider_instance_mismatch",
        );
        let mut wrong = call.call.clone();
        wrong.nonce = 1;
        shell_error(
            client
                .execute_signed(resign(wrong, 2), NOW)
                .unwrap()
                .result()
                .await,
            "nonce_conflict",
        );
        shell_error(
            client
                .execute_directory_with_inputs(
                    call.clone(),
                    ExecutionInputs {
                        version: 2,
                        ..inputs(1)
                    },
                )
                .unwrap()
                .result()
                .await,
            "invalid_execution_inputs",
        );
        let unsupported = signed(
            2,
            0,
            2,
            ProviderCommand::CreateProject(CreateProjectRequest {
                title: "Pending M5".into(),
                description: String::new(),
            }),
        );
        assert!(matches!(
            client
                .execute_signed(unsupported, NOW)
                .unwrap()
                .result()
                .await,
            Err(Error::IncompleteCoverage)
        ));
        let mut malformed = call;
        let ProviderCommand::RegisterWorker(request) = &mut malformed.call.command else {
            unreachable!()
        };
        request.display_name.clear();
        shell_error(
            client
                .execute_signed(malformed, NOW)
                .unwrap()
                .result()
                .await,
            "invalid_contract",
        );
        assert_eq!(db.stored().await, before);
        assert_eq!(shell(&client).await.nonce(account(2)), 0);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn domain_failure_commits_only_failed_receipt_and_invariant_failure_commits_nothing() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let before = db.stored().await;
        let failed = client
            .execute_signed(signed(2, 0, 1, calendar(37)), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            failed.outcome,
            ExecutionOutcome::Failed("worker_not_found".into())
        );
        assert_eq!(db.stored().await.1, before.1);
        assert_eq!(shell(&client).await.nonce(account(2)), 1);
        assert!(shell(&client).await.events.is_empty());
        client
            .execute_directory_with_inputs(signed(2, 1, 2, register()), inputs(10))
            .unwrap()
            .result()
            .await
            .unwrap();
        let before = db.stored().await;
        assert!(matches!(
            client
                .execute_directory_with_inputs(signed(3, 0, 3, register()), inputs(10))
                .unwrap()
                .result()
                .await,
            Err(Error::Invariant("invalid_worker_state"))
        ));
        assert_eq!(db.stored().await, before);
        assert_eq!(shell(&client).await.nonce(account(3)), 0);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn sql_failure_rolls_back_new_child_contract_shell_and_retry() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let before = db.stored().await;
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER reject_commit BEFORE UPDATE ON candidate_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
        let call = signed(2, 0, 1, register());
        assert!(matches!(
            client
                .execute_directory_with_inputs(call.clone(), inputs(10))
                .unwrap()
                .result()
                .await,
            Err(Error::Storage(_))
        ));
        assert_eq!(db.stored().await, before);
        assert!(projection(&client).await.3.is_empty());
        sqlx::query("DROP TRIGGER reject_commit")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert_eq!(
            client
                .execute_directory_with_inputs(call, inputs(10))
                .unwrap()
                .result()
                .await
                .unwrap()
                .outcome,
            ExecutionOutcome::Success
        );
        assert_eq!(projection(&client).await.3.len(), 1);
        assert_eq!(shell(&client).await.events.len(), 1);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn competing_workers_replay_one_commit_and_reject_conflicting_nonce() {
    test(async {
        let db = Database::new();
        let first = db.start().await;
        let second = db.start().await;
        let call = signed(2, 0, 1, register());
        let a = first
            .client()
            .execute_directory_with_inputs(call.clone(), inputs(10))
            .unwrap();
        let b = second
            .client()
            .execute_directory_with_inputs(call, inputs(11))
            .unwrap();
        assert_eq!(a.result().await.unwrap(), b.result().await.unwrap());
        assert_eq!(db.stored().await.0, 1);
        let a = first
            .client()
            .execute_signed(signed(2, 1, 2, calendar(37)), NOW)
            .unwrap();
        let b = second
            .client()
            .execute_signed(signed(2, 1, 3, calendar(83)), NOW)
            .unwrap();
        let results = [a.result().await, b.result().await];
        assert_eq!(results.iter().filter(|v| v.is_ok()).count(), 1);
        shell_error(
            results.into_iter().find(Result::is_err).unwrap(),
            "nonce_conflict",
        );
        let state = shell(&first.client()).await;
        assert_eq!(state.nonce(account(2)), 2);
        assert_eq!(state.events.len(), 2);
        assert_eq!(digest(&state), digest(&shell(&second.client()).await));
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
    });
}

#[test]
fn startup_checks_fixture_identity_mode_and_contract_pointer() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        worker.shutdown().await.unwrap();
        let before = db.stored().await;
        let mut wrong = fixture(GAS);
        wrong.reference_controller = account(3);
        assert!(
            Worker::start_directory_fixture(db.0.clone(), wrong)
                .await
                .is_err()
        );
        let mut wrong = fixture(GAS);
        wrong.catalog.0[1].name = "Changed".into();
        assert!(
            Worker::start_directory_fixture(db.0.clone(), wrong)
                .await
                .is_err()
        );
        assert!(Worker::start(db.0.clone(), vec![], vec![]).await.is_err());
        assert_eq!(db.stored().await, before);
        let pool = db.pool().await;
        let mut stored: Value = serde_json::from_slice(&before.2).unwrap();
        stored["directory"] = json!(Account::new([55; 32]));
        sqlx::query("UPDATE candidate_state SET shell = ?")
            .bind(serde_json::to_vec(&stored).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert!(
            Worker::start_directory_fixture(db.0.clone(), fixture(GAS))
                .await
                .is_err()
        );
        assert_eq!(db.stored().await.1, before.1);
    });
}

#[test]
fn metered_failure_commits_failed_receipt_without_business_changes() {
    test(async {
        let db = Database::new();
        let worker =
            Worker::start_directory_fixture(db.0.clone(), fixture(Weight::from_parts(1, 1)))
                .await
                .unwrap();
        let before = db.stored().await;
        let receipt = worker
            .client()
            .execute_signed(signed(2, 0, 1, register()), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            receipt.outcome,
            ExecutionOutcome::Failed("sandbox_out_of_gas".into())
        );
        assert_eq!(db.stored().await.1, before.1);
        assert_eq!(shell(&worker.client()).await.nonce(account(2)), 1);
        assert!(shell(&worker.client()).await.events.is_empty());
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn signed_score_policy_uses_root_authority_shared_receipts_and_exact_policy_bytes() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let policy = ScorePolicy::new(Percentage::new(50).unwrap(), Percentage::new(50).unwrap())
            .unwrap()
            .with_coordinator_vote_weights(7, 3)
            .unwrap();
        let denied = client
            .execute_signed(
                signed(2, 0, 1, ProviderCommand::SetScorePolicy(policy)),
                NOW,
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            denied.outcome,
            ExecutionOutcome::Failed("system_origin_required".into())
        );
        assert!(shell(&client).await.events.is_empty());
        let call = signed(1, 0, 2, ProviderCommand::SetScorePolicy(policy));
        let receipt = client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt.outcome, ExecutionOutcome::Success);
        let state = shell(&client).await;
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].kind, DomainEventKind::ScorePolicyUpdated);
        assert_eq!(state.events[0].origin, Some(account(1)));
        assert_eq!(state.nonce(account(2)), 1);
        assert_eq!(state.nonce(account(1)), 1);
        let before = db.stored().await;
        let persisted: Value = serde_json::from_slice(&before.2).unwrap();
        let address: Account = serde_json::from_value(persisted["directory"].clone()).unwrap();
        let query = client
            .query(ink_v5_compatibility::worker::Invocation {
                caller: Account::new(account(1).into_bytes()),
                action: ink_v5_compatibility::worker::Action::Call { address },
                input: blake2_256(b"score_policy")[..4].to_vec(),
                gas: GAS,
                timestamp_ms: 100_000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            Result::<ScorePolicy, u8>::decode_all(&mut &query.data[..])
                .unwrap()
                .unwrap(),
            policy
        );
        assert_eq!(
            client
                .execute_signed(call, UnixSeconds::new(999))
                .unwrap()
                .result()
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(db.stored().await, before);
        worker.shutdown().await.unwrap();
    });
}
