//! Full retained-state comparison, fresh sealed imports and current-state export.
use generated_contracts::*;
use ink_v5_compatibility::{
    GAS,
    worker::{Error, WholeConfiguration, Worker},
};
use parity_scale_codec::{DecodeAll, Encode};
use provider_execution::migration::ArchiveV1;
use serde::Deserialize;
use sp_core::{H256, hashing::sha2_256};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    archives: Vec<Retained>,
    digests: Vec<String>,
    priority_cases: Vec<serde_json::Value>,
    aggregate_cases: Vec<serde_json::Value>,
    planning_cases: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Retained {
    before_step: usize,
    archive: String,
}
fn reference() -> Reference {
    serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap()
}
fn config(archive: &ArchiveV1) -> WholeConfiguration {
    WholeConfiguration {
        root: archive.checkpoint.info.root_account,
        generation: archive.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: GAS,
    }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    async fn stored(&self) -> (i64, Vec<u8>, Vec<u8>) {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", self.0.display()))
            .await
            .unwrap();
        let row = sqlx::query_as("SELECT revision,runtime,shell FROM candidate_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        row
    }
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ink-import-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_file(&self.0).expect("remove fixture database");
        }
    }
}
fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
#[test]
fn retained_archives_import_exactly_restart_and_never_replace_populated_stores() {
    run(async {
        for retained in reference().archives {
            let archive: ArchiveV1 = serde_json::from_str(&retained.archive).unwrap();
            let configuration = config(&archive);
            let db = Database::new();
            let worker =
                Worker::import_archive(db.0.clone(), archive.clone(), configuration.clone())
                    .await
                    .unwrap_or_else(|e| panic!("import before step {}: {e}", retained.before_step));
            let before = db.stored().await;
            let exported = worker.client().export_archive().unwrap().result().await;
            worker.shutdown().await.unwrap();
            assert_eq!(before, db.stored().await, "read-only export");
            assert_assets(&before, &archive);
            assert_eq!(
                serde_json::to_vec(&exported.unwrap()).unwrap(),
                serde_json::to_vec(&archive).unwrap(),
                "before step {}",
                retained.before_step
            );
            assert!(
                Worker::import_archive(db.0.clone(), archive.clone(), configuration.clone())
                    .await
                    .is_err()
            );
            let reopened = Worker::reopen(db.0.clone(), configuration).await.unwrap();
            let exported = reopened.client().export_archive().unwrap().result().await;
            reopened.shutdown().await.unwrap();
            assert_eq!(
                serde_json::to_vec(&exported.unwrap()).unwrap(),
                serde_json::to_vec(&archive).unwrap()
            );
        }
    })
}
#[test]
fn complete_shadow_trace_preserves_current_sandbox_writes_for_rollback() {
    run(async {
        let reference = reference();
        let retained = reference
            .archives
            .iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        let archive: ArchiveV1 = serde_json::from_str(&retained.archive).unwrap();
        let db = Database::new();
        let configuration = config(&archive);
        let worker = Worker::import_archive(db.0.clone(), archive, configuration.clone())
            .await
            .unwrap();
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        for (index, step) in trace["steps"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .skip(70)
        {
            let result = worker
                .client()
                .execute_directory_with_inputs(
                    serde_json::from_value(step["signed"].clone()).unwrap(),
                    serde_json::from_value(step["inputs"].clone()).unwrap(),
                )
                .unwrap()
                .result()
                .await;
            if let Some(receipt) = step["outcome"].get("receipt") {
                assert_eq!(
                    serde_json::to_value(result.unwrap()).unwrap(),
                    *receipt,
                    "outcome step {index}"
                );
            } else {
                assert!(
                    matches!(result, Err(Error::Invariant(_))),
                    "infrastructure outcome step {index}: {result:?}"
                );
            }
            let exported = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                format!(
                    "{:x}",
                    H256::from(sha2_256(&serde_json::to_vec(&exported).unwrap()))
                ),
                reference.digests[index],
                "complete archive after step {index}"
            );
        }
        let current = worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        let expected: ArchiveV1 =
            serde_json::from_str(&reference.archives.last().unwrap().archive).unwrap();
        use ink_v5_compatibility::worker::comparison::*;
        let last: SignedContractCallV1 = serde_json::from_value(
            trace["steps"].as_array().unwrap().last().unwrap()["signed"].clone(),
        )
        .unwrap();
        let expected_receipt = &expected.checkpoint.receipts[&last.call.operation_id].receipt;
        let actual_receipt = &current.checkpoint.receipts[&last.call.operation_id].receipt;
        assert!(matches!(
            compare(
                Observation {
                    archive: Some(&expected),
                    outcome: ObservedOutcome::Receipt(expected_receipt)
                },
                Observation {
                    archive: Some(&current),
                    outcome: ObservedOutcome::Receipt(actual_receipt)
                }
            )
            .unwrap(),
            Comparison::Agreement { .. }
        ));
        assert_assets(&db.stored().await, &current);
        worker.shutdown().await.unwrap();
        let reopened = Worker::reopen(db.0.clone(), configuration).await.unwrap();
        let restarted = reopened.client().export_archive().unwrap().result().await;
        reopened.shutdown().await.unwrap();
        assert_eq!(
            serde_json::to_vec(&current).unwrap(),
            serde_json::to_vec(&restarted.unwrap()).unwrap()
        );
        if let Ok(path) = std::env::var("SANDBOX_ROLLBACK_OUTPUT") {
            std::fs::write(path, serde_json::to_vec(&current).unwrap()).unwrap();
        }
    })
}
#[test]
fn missing_incompatible_and_failed_imports_never_regenerate_state() {
    run(async {
        let original: ArchiveV1 = serde_json::from_str(&reference().archives[0].archive).unwrap();
        let configuration = config(&original);
        let db = Database::new();
        assert!(
            Worker::reopen(db.0.clone(), configuration.clone())
                .await
                .is_err()
        );
        let mut invalid = original.clone();
        invalid.version = 2;
        assert!(
            Worker::import_archive(db.0.clone(), invalid, configuration.clone())
                .await
                .is_err()
        );
        let worker = Worker::import_archive(db.0.clone(), original, configuration.clone())
            .await
            .unwrap();
        worker.shutdown().await.unwrap();
        let mut incompatible = configuration.clone();
        incompatible.mock_funding = false;
        assert!(Worker::reopen(db.0.clone(), incompatible).await.is_err());
        Worker::reopen(db.0.clone(), configuration)
            .await
            .unwrap()
            .shutdown()
            .await
            .unwrap();
    })
}

#[test]
fn retained_score_overflow_precedes_missing_modern_worker_ratings() {
    run(async {
        for case in reference().priority_cases {
            let archive: ArchiveV1 =
                serde_json::from_str(case["archive"].as_str().unwrap()).unwrap();
            let configuration = config(&archive);
            let db = Database::new();
            let worker = Worker::import_archive(db.0.clone(), archive, configuration)
                .await
                .unwrap();
            let result = worker
                .client()
                .execute_directory_with_inputs(
                    serde_json::from_value(case["signed"].clone()).unwrap(),
                    serde_json::from_value(case["inputs"].clone()).unwrap(),
                )
                .unwrap()
                .result()
                .await;
            if let Some(receipt) = case["outcome"].get("receipt") {
                assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), *receipt);
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Invariant("provider_unavailable"))
                ));
            }
            let current = worker.client().export_archive().unwrap().result().await;
            worker.shutdown().await.unwrap();
            assert_eq!(
                format!(
                    "{:x}",
                    H256::from(sha2_256(&serde_json::to_vec(&current.unwrap()).unwrap()))
                ),
                case["afterDigest"].as_str().unwrap()
            );
        }
    })
}

fn assert_assets(stored: &(i64, Vec<u8>, Vec<u8>), archive: &ArchiveV1) {
    use frame_support::traits::fungibles::Inspect;
    use ink_v5_compatibility::{Account, Assets, Contracts, checkpoint::Checkpoint};
    let shell: serde_json::Value = serde_json::from_slice(&stored.2).unwrap();
    let directory: Account = serde_json::from_value(shell["directory"].clone()).unwrap();
    let mut ext = Checkpoint::decode(&stored.1).unwrap().restore().unwrap();
    ext.execute_with(|| {
        let mut outside = 0u128;
        let mut visible = 0u128;
        for (owner, amount) in &archive.checkpoint.balances {
            let actual =
                <Assets as Inspect<Account>>::balance(1, &Account::new(owner.into_bytes()));
            assert_eq!(actual, u128::from(amount.units()) + 1);
            outside += actual;
            visible += u128::from(amount.units());
        }
        for (id, project) in &archive.checkpoint.projects {
            let bytes = Contracts::get_storage(directory.clone(), (8u32, *id, 1u32).encode())
                .unwrap()
                .unwrap();
            let page = Vec::<u8>::decode_all(&mut &bytes[..]).unwrap();
            let address = Account::decode_all(&mut &page[..]).unwrap();
            let escrow = u128::from(project.planning.escrow.units())
                + u128::from(project.execution_escrow.units());
            let actual = <Assets as Inspect<Account>>::balance(1, &address);
            assert_eq!(actual, escrow + 1);
            outside += actual;
            visible += escrow;
        }
        let total = u128::from(u64::MAX) * 2;
        assert_eq!(<Assets as Inspect<Account>>::total_issuance(1), total);
        assert_eq!(
            <Assets as Inspect<Account>>::balance(1, &directory) + outside,
            total
        );
        visible += archive
            .checkpoint
            .bramp
            .withdrawals
            .values()
            .filter(|w| w.status == WithdrawalStatus::Pending)
            .map(|w| u128::from(w.amount.money().units()))
            .sum::<u128>();
        assert_eq!(visible, u128::from(archive.checkpoint.minted_units.units()));
    });
}
#[test]
fn sealed_import_messages_reject_root_and_other_accounts_without_effects() {
    run(async {
        use ink_v5_compatibility::{
            Account,
            worker::{Action, Invocation, Status},
        };
        let archive: ArchiveV1 = serde_json::from_str(&reference().archives[0].archive).unwrap();
        let configuration = config(&archive);
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive, configuration.clone())
            .await
            .unwrap();
        let before = db.stored().await;
        let state: serde_json::Value = serde_json::from_slice(&before.2).unwrap();
        let directory: Account = serde_json::from_value(state["directory"].clone()).unwrap();
        for origin in [configuration.root, AccountId32::from_bytes([8; 32])] {
            for (name, args) in [
                ("begin_import", Vec::new()),
                ("seal_import", Vec::new()),
                ("import_record_page", (0u32, vec![5u8, 1], true).encode()),
            ] {
                let mut input = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
                input.extend(args);
                let outcome = worker
                    .client()
                    .query(Invocation {
                        caller: Account::new(origin.into_bytes()),
                        action: Action::Call {
                            address: directory.clone(),
                        },
                        input,
                        gas: GAS,
                        timestamp_ms: 100_000,
                    })
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
                assert_eq!(outcome.status, Status::Reverted);
                let rejected = Result::<Result<(), String>, u8>::decode_all(&mut &outcome.data[..])
                    .unwrap()
                    .unwrap()
                    .unwrap_err();
                assert_eq!(
                    rejected,
                    if origin == configuration.root {
                        "invalid_archive"
                    } else {
                        "system_origin_required"
                    }
                );
            }
        }
        worker.shutdown().await.unwrap();
        assert_eq!(before, db.stored().await);
    })
}

#[test]
fn retained_large_profile_is_completely_importable_and_exportable() {
    run(async {
        let mut archive: ArchiveV1 =
            serde_json::from_str(&reference().archives[1].archive).unwrap();
        archive
            .checkpoint
            .workers
            .values_mut()
            .next()
            .unwrap()
            .display_name = "n".repeat(MAX_SIGNABLE_BYTES - 200);
        let configuration = config(&archive);
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive.clone(), configuration)
            .await
            .unwrap();
        let current = worker.client().export_archive().unwrap().result().await;
        worker.shutdown().await.unwrap();
        assert_eq!(
            serde_json::to_vec(&current.unwrap()).unwrap(),
            serde_json::to_vec(&archive).unwrap()
        );
    })
}

async fn retained_case(case: serde_json::Value, output: Option<&str>) {
    let archive: ArchiveV1 = serde_json::from_str(case["archive"].as_str().unwrap()).unwrap();
    let configuration = config(&archive);
    let db = Database::new();
    let worker = Worker::import_archive(db.0.clone(), archive.clone(), configuration)
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", case["label"]));
    assert_assets(&db.stored().await, &archive);
    let result = worker
        .client()
        .execute_directory_with_inputs(
            serde_json::from_value(case["signed"].clone()).unwrap(),
            serde_json::from_value(case["inputs"].clone()).unwrap(),
        )
        .unwrap()
        .result()
        .await;
    assert_eq!(
        serde_json::to_value(result.unwrap()).unwrap(),
        case["outcome"]["receipt"],
        "{}",
        case["label"]
    );
    let current = worker
        .client()
        .export_archive()
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_assets(&db.stored().await, &current);
    worker.shutdown().await.unwrap();
    let bytes = serde_json::to_vec(&current).unwrap();
    assert_eq!(
        format!("{:x}", H256::from(sha2_256(&bytes))),
        case["afterDigest"].as_str().unwrap(),
        "{}",
        case["label"]
    );
    if let Some(output) = output
        && let Ok(path) = std::env::var(output)
    {
        std::fs::write(path, bytes).unwrap();
    }
}
#[test]
fn retained_aggregates_above_the_single_call_limit_keep_working() {
    run(async {
        for case in reference().aggregate_cases {
            retained_case(case, None).await;
        }
    })
}
#[test]
fn retained_positive_planning_escrow_pays_and_freezes_with_complete_backing() {
    run(async {
        for (index, case) in reference().planning_cases.into_iter().enumerate() {
            retained_case(
                case,
                Some(if index == 0 {
                    "SANDBOX_PLANNING_ROLLBACK_OUTPUT"
                } else {
                    "SANDBOX_FROZEN_PLANNING_ROLLBACK_OUTPUT"
                }),
            )
            .await;
        }
    })
}
