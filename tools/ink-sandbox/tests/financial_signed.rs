//! Native and WASM independently replay identical signed calls and captured inputs.
use frame_support::{traits::fungibles::Inspect, weights::Weight};
use generated_contracts::*;
use ink_sandbox::{
    Account, Assets, Contracts, GAS,
    checkpoint::Checkpoint,
    worker::{Client, DirectoryFixture, Error, FinancialProjection, MarketplaceProjection, Worker},
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
        "../../../services/mock-provider/src/domain/fixtures/financial-signed-v5.json"
    ))
    .unwrap()
}
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn fixture(gas: Weight) -> DirectoryFixture {
    let initial: MarketplaceProjection =
        serde_json::from_value(trace()["initial"][0].clone()).unwrap();
    DirectoryFixture {
        root: account(1),
        funded_accounts: (2..=5).map(account).collect(),
        catalog: (initial.0.roles, initial.0.skills, initial.0.skill_roles),
        reservation_controller: account(4),
        reference_controller: account(4),
        generation: Some(ProviderInstanceId::from_bytes([109; 16])),
        gas_limit: gas,
        projects: true,
        money: Some(true),
    }
}
fn signed(seed: u8, nonce: u64, op: u8, command: ProviderCommand) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        ProviderInstanceId::from_bytes([109; 16]),
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
            "ink-financial-signed-{}-{}.sqlite",
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
type Projection = FinancialProjection;
async fn projection(client: &Client) -> Projection {
    client
        .financial_projection(account(1))
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
            for probe in trace["temporalProbes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|probe| probe["beforeStep"] == index)
            {
                let before = db.stored().await;
                let result = worker
                    .client()
                    .execute_directory_with_inputs(
                        serde_json::from_value(probe["signed"].clone()).unwrap(),
                        serde_json::from_value(probe["inputs"].clone()).unwrap(),
                    )
                    .unwrap()
                    .result()
                    .await;
                let Err(Error::Invariant(code)) = result else {
                    panic!("temporal probe {index}: {result:?}")
                };
                assert_eq!(
                    json!({"error":code}),
                    probe["outcome"],
                    "temporal probe {index}"
                );
                assert_eq!(
                    db.stored().await,
                    before,
                    "temporal probe {index} committed"
                );
            }
            let call: SignedContractCallV1 =
                serde_json::from_value(step["signed"].clone()).unwrap();
            let inputs: ExecutionInputs = serde_json::from_value(step["inputs"].clone()).unwrap();
            if index == 61 {
                // First legacy URL: a failed durable commit must retain neither
                // the contract hash nor its tentative offchain preimage.
                let before = db.stored().await;
                let pool = db.pool().await;
                sqlx::query("CREATE TRIGGER reject_evidence BEFORE UPDATE ON candidate_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
                assert!(matches!(
                    worker
                        .client()
                        .execute_directory_with_inputs(call.clone(), inputs.clone())
                        .unwrap()
                        .result()
                        .await,
                    Err(Error::Storage(_))
                ));
                assert_eq!(db.stored().await, before);
                sqlx::query("DROP TRIGGER reject_evidence")
                    .execute(&pool)
                    .await
                    .unwrap();
                pool.close().await;
                worker.shutdown().await.unwrap();
                worker = db.start().await;
                assert_eq!(db.stored().await, before);
            }
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
            let stored = db.stored().await;
            let projected = projection(&worker.client()).await;
            assert_assets(&stored, &projected);
            if index == 8 {
                assert_eq!(
                    projected
                        .1
                        .0
                        .iter()
                        .find(|(owner, _)| *owner == account(4))
                        .unwrap()
                        .1,
                    Money::ZERO
                );
            }
            if index == 8 || index == 14 || index == 45 || index == 70 || index == 95 {
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
#[test]
fn confirmation_sql_failure_and_competing_full_balance_withdrawals_preserve_supply() {
    test(async {
        let db = Database::new();
        let first = db.start().await;
        let client = first.client();
        for index in 0..5 {
            execute_step(&client, index).await.unwrap();
        }
        let before = db.stored().await;
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER reject_commit BEFORE UPDATE ON candidate_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
        assert!(matches!(
            execute_step(&client, 5).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(db.stored().await, before);
        sqlx::query("DROP TRIGGER reject_commit")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert_eq!(
            json!(execute_step(&client, 5).await.unwrap()),
            trace()["steps"][5]["outcome"]["receipt"]
        );
        let second = db.start().await;
        let withdrawal = ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
            amount: KvnAmount::new(Money::new(101)).unwrap(),
        });
        let nonce = shell(&client).await.nonce(account(4));
        let left = client
            .execute_directory_with_inputs(signed(4, nonce, 240, withdrawal.clone()), inputs(50))
            .unwrap();
        let right = second
            .client()
            .execute_directory_with_inputs(signed(4, nonce, 241, withdrawal), inputs(51))
            .unwrap();
        let results = [left.result().await, right.result().await];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let mut winner = None;
        for result in results {
            match result {
                Ok(receipt) => {
                    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
                    winner = receipt.created_entity_id;
                }
                Err(Error::Shell(error)) => assert_eq!(error.code(), "nonce_conflict"),
                other => panic!("{other:?}"),
            }
        }
        let projected = projection(&client).await;
        assert_eq!(
            projected
                .1
                .0
                .iter()
                .find(|(owner, _)| *owner == account(4))
                .unwrap()
                .1,
            Money::ZERO
        );
        assert_eq!(
            projected
                .1
                .3
                .iter()
                .filter(|record| record.status == WithdrawalStatus::Pending)
                .count(),
            1
        );
        let stored = db.stored().await;
        let mut ext = Checkpoint::decode(&stored.1).unwrap().restore().unwrap();
        ext.execute_with(|| {
            assert_eq!(
                <Assets as Inspect<Account>>::balance(1, &Account::new(account(4).into_bytes())),
                1
            )
        });
        first.shutdown().await.unwrap();
        second.shutdown().await.unwrap();
        let worker = db.start().await;
        let client = worker.client();
        let root_nonce = shell(&client).await.nonce(account(1));
        client
            .execute_signed(
                signed(
                    1,
                    root_nonce,
                    242,
                    ProviderCommand::CancelWithdrawal {
                        withdrawal_id: winner.unwrap(),
                    },
                ),
                NOW,
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        let projected = projection(&client).await;
        assert_eq!(
            projected
                .1
                .0
                .iter()
                .find(|(owner, _)| *owner == account(4))
                .unwrap()
                .1,
            Money::new(101)
        );
        assert_eq!(projected.1.1, Money::new(138));
        worker.shutdown().await.unwrap();
    });
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = blake2_256(name.as_bytes())[..4].to_vec();
    data.extend(args.encode());
    data
}
#[test]
fn normal_ramp_funding_permissions_private_reads_and_queries_do_not_bypass_accounting() {
    use ink_sandbox::worker::{Action, Invocation, Status};
    test(async {
        let db = Database::new();
        let mut configured = fixture(GAS);
        configured.money = Some(false);
        let worker = Worker::start_directory_fixture(db.0.clone(), configured)
            .await
            .unwrap();
        let client = worker.client();
        let failed = client
            .execute_signed(
                signed(
                    1,
                    0,
                    200,
                    ProviderCommand::FundAccount(FundAccountRequest {
                        account: account(2),
                        amount: Money::new(37),
                    }),
                ),
                NOW,
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            failed.outcome,
            ExecutionOutcome::Failed("mock_funding_disabled".into())
        );
        assert!(
            client
                .money_projection(account(1))
                .unwrap()
                .result()
                .await
                .unwrap()
                .0
                .is_empty()
        );
        let created = client
            .execute_signed(
                signed(
                    2,
                    0,
                    201,
                    ProviderCommand::CreateDeposit(CreateDepositRequest {
                        amount: KvnAmount::new(Money::new(37)).unwrap(),
                    }),
                ),
                NOW,
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        let id = created.created_entity_id.unwrap();
        client
            .execute_signed(
                signed(
                    1,
                    1,
                    202,
                    ProviderCommand::ConfirmDeposit { deposit_id: id },
                ),
                NOW,
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        let before = db.stored().await;
        let persisted: Value = serde_json::from_slice(&before.2).unwrap();
        let directory: Account = serde_json::from_value(persisted["directory"].clone()).unwrap();
        for (caller, data, error) in [
            (3, input("deposit", id), "deposit_owner_or_system_required"),
            (
                2,
                input("financial_projection", ()),
                "system_origin_required",
            ),
            (
                2,
                input("book_debit", (account(2), 1u64)),
                "registered_project_required",
            ),
            (
                2,
                input("book_credit", (account(2), 1u64)),
                "registered_project_required",
            ),
            (
                1,
                input("configure_money", true),
                "money_configuration_fixed",
            ),
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
            // Err layout does not depend on the successful return type.
            assert_eq!(
                Result::<Result<(), String>, u8>::decode_all(&mut &outcome.data[..])
                    .unwrap()
                    .unwrap(),
                Err(error.to_string())
            );
        }
        let queried = client
            .query(Invocation {
                caller: Account::new(account(2).into_bytes()),
                action: Action::Call { address: directory },
                input: input("create_withdrawal", (37u64, [42u8; 32])),
                gas: GAS,
                timestamp_ms: 100_000,
            })
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(queried.status, Status::Success);
        assert_eq!(db.stored().await, before);
        assert_eq!(
            client
                .money_projection(account(1))
                .unwrap()
                .result()
                .await
                .unwrap()
                .0,
            vec![(account(2), Money::new(37))]
        );
        worker.shutdown().await.unwrap();
    });
}

fn assert_assets(stored: &(i64, Vec<u8>, Vec<u8>, Vec<u8>), projected: &Projection) {
    use pallet_contracts::{CollectEvents, DebugInfo, Determinism};
    let persisted: Value = serde_json::from_slice(&stored.2).unwrap();
    let directory: Account = serde_json::from_value(persisted["directory"].clone()).unwrap();
    let mut ext = Checkpoint::decode(&stored.1).unwrap().restore().unwrap();
    ext.execute_with(|| {
        let mut outside = 0u128;
        for (owner, visible) in &projected.1.0 {
            let actual =
                <Assets as Inspect<Account>>::balance(1, &Account::new(owner.into_bytes()));
            assert_eq!(actual, u128::from(visible.units()) + 1);
            outside = outside.checked_add(actual).unwrap();
        }
        let registry = Contracts::bare_call(
            Account::new(account(1).into_bytes()),
            directory.clone(),
            0,
            GAS,
            None,
            input("projects", ()),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        )
        .result
        .unwrap();
        assert!(!registry.did_revert());
        let addresses = Result::<Vec<(EntityId, Account)>, u8>::decode_all(&mut &registry.data[..])
            .unwrap()
            .unwrap();
        for (id, address) in addresses {
            let project = projected.0.3.iter().find(|p| p.project_id == id).unwrap();
            let escrow = u128::from(project.planning.escrow.units())
                + u128::from(project.execution_escrow.units());
            let actual = <Assets as Inspect<Account>>::balance(1, &address);
            assert_eq!(actual, escrow + 1);
            outside = outside.checked_add(actual).unwrap();
        }
        let total = u128::from(u64::MAX) * 2;
        assert_eq!(<Assets as Inspect<Account>>::total_issuance(1), total);
        assert_eq!(
            <Assets as Inspect<Account>>::balance(1, &directory).checked_add(outside),
            Some(total)
        );
    });
    let free = projected
        .1
        .0
        .iter()
        .map(|(_, amount)| u128::from(amount.units()))
        .sum::<u128>();
    let escrow = projected
        .0
        .3
        .iter()
        .map(|p| u128::from(p.planning.escrow.units()) + u128::from(p.execution_escrow.units()))
        .sum::<u128>();
    let holds = projected
        .1
        .3
        .iter()
        .filter(|w| w.status == WithdrawalStatus::Pending)
        .map(|w| u128::from(w.amount.money().units()))
        .sum::<u128>();
    assert_eq!(free + escrow + holds, u128::from(projected.1.1.units()));
}
#[test]
fn settlement_sql_failure_discards_all_payouts_activation_and_receipt_then_retries_after_restart() {
    test(async {
        let db = Database::new();
        let worker = db.start().await;
        let client = worker.client();
        let trace = trace();
        let steps = trace["steps"].as_array().unwrap();
        let target = steps
            .iter()
            .position(|step| {
                step["signed"]["call"]["command"]["type"] == "AcceptMilestoneDelivery"
                    && step["outcome"]["receipt"]["outcome"]["type"] == "Success"
            })
            .unwrap();
        for (index, step) in steps.iter().enumerate().take(target) {
            let observed = match execute_step(&client, index).await {
                Ok(receipt) => json!({"receipt":receipt}),
                Err(Error::Shell(error)) => json!({"error":error.code()}),
                Err(Error::Invariant(code)) => json!({"error":code}),
                other => panic!("step {index}: {other:?}"),
            };
            assert_eq!(observed, step["outcome"]);
        }
        let before = db.stored().await;
        let projected = projection(&client).await;
        assert_assets(&before, &projected);
        let pool = db.pool().await;
        sqlx::query("CREATE TRIGGER reject_commit BEFORE UPDATE ON candidate_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
        assert!(matches!(
            execute_step(&client, target).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(db.stored().await, before);
        assert_eq!(projection(&client).await.encode(), projected.encode());
        assert_assets(&db.stored().await, &projected);
        worker.shutdown().await.unwrap();
        sqlx::query("DROP TRIGGER reject_commit")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let worker = db.start().await;
        let client = worker.client();
        assert_eq!(db.stored().await, before);
        assert_eq!(
            json!(execute_step(&client, target).await.unwrap()),
            steps[target]["outcome"]["receipt"]
        );
        let after = projection(&client).await;
        assert_eq!(hex(sha2_256(&after.encode())), steps[target]["stateDigest"]);
        assert_assets(&db.stored().await, &after);
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn metered_failure_during_execution_approval_discards_funding_and_reservations() {
    use ink_sandbox::worker::Outcome;
    test(async {
        let trace = trace();
        let steps = trace["steps"].as_array().unwrap();
        let target = steps
            .iter()
            .position(|step| {
                step["signed"]["call"]["command"]["type"] == "ApproveExecution"
                    && step["outcome"]["receipt"]["outcome"]["type"] == "Success"
            })
            .unwrap();
        let measured = Database::new();
        let worker = measured.start().await;
        let mut setup = 0u64;
        for index in 0..target {
            execute_step(&worker.client(), index).await.unwrap();
            let row = measured.stored().await;
            let outcome = Outcome::decode_all(&mut &row.3[..]).unwrap();
            setup = setup.max(outcome.gas_consumed.ref_time());
        }
        execute_step(&worker.client(), target).await.unwrap();
        let row = measured.stored().await;
        let approval = Outcome::decode_all(&mut &row.3[..])
            .unwrap()
            .gas_consumed
            .ref_time();
        assert!(
            approval > setup,
            "approval fixture must cost more than setup: {approval} <= {setup}"
        );
        let budget = setup.checked_add((approval - setup) / 2).unwrap();
        worker.shutdown().await.unwrap();
        let db = Database::new();
        let worker = Worker::start_directory_fixture(
            db.0.clone(),
            fixture(Weight::from_parts(budget, GAS.proof_size())),
        )
        .await
        .unwrap();
        let client = worker.client();
        for index in 0..target {
            execute_step(&client, index).await.unwrap();
        }
        let before = db.stored().await;
        let projected = projection(&client).await;
        let receipt = execute_step(&client, target).await.unwrap();
        // A nested ink call can turn callee gas exhaustion into an outer trap.
        // The measured input succeeds at full gas; only this budget is changed.
        assert!(matches!(&receipt.outcome,ExecutionOutcome::Failed(code)
            if matches!(code.as_str(),"sandbox_out_of_gas"|"sandbox_contract_trapped")));
        assert_eq!(db.stored().await.1, before.1);
        assert_eq!(projection(&client).await.encode(), projected.encode());
        assert_assets(&db.stored().await, &projected);
        assert!(receipt.first_event_cursor.is_none());
        worker.shutdown().await.unwrap();
    });
}
