//! Full retained-state comparison, fresh sealed imports and current-state export.
use generated_contracts::*;
use ink_v5_compatibility::{
    GAS,
    worker::{Error, WholeConfiguration, Worker},
};
use parity_scale_codec::{DecodeAll, Encode};
use provider_execution::migration::CoreArchiveV4;
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
fn config(archive: &CoreArchiveV4) -> WholeConfiguration {
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
            let archive: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
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
        let archive: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
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
        let expected: CoreArchiveV4 =
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
        let original: CoreArchiveV4 =
            serde_json::from_str(&reference().archives[0].archive).unwrap();
        let configuration = config(&original);
        let db = Database::new();
        assert!(
            Worker::reopen(db.0.clone(), configuration.clone())
                .await
                .is_err()
        );
        let mut invalid = original.clone();
        invalid.version = 5;
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
            let archive: CoreArchiveV4 =
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

fn assert_assets(stored: &(i64, Vec<u8>, Vec<u8>), archive: &CoreArchiveV4) {
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
        let archive: CoreArchiveV4 =
            serde_json::from_str(&reference().archives[0].archive).unwrap();
        let configuration = config(&archive);
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive, configuration.clone())
            .await
            .unwrap();
        let before = db.stored().await;
        let state: serde_json::Value = serde_json::from_slice(&before.2).unwrap();
        let directory: Account = serde_json::from_value(state["directory"].clone()).unwrap();
        for origin in [configuration.root, AccountId32::from_bytes([8; 32])] {
            for (name, args, expected) in [
                (
                    "begin_import",
                    Vec::new(),
                    if origin == configuration.root {
                        "invalid_archive"
                    } else {
                        "system_origin_required"
                    },
                ),
                ("seal_import", Vec::new(), "record_in_progress"),
                (
                    "import_record_page",
                    (0u32, vec![5u8, 1], true).encode(),
                    "import_closed",
                ),
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
                assert_eq!(rejected, expected);
            }
        }
        worker.shutdown().await.unwrap();
        assert_eq!(before, db.stored().await);
    })
}

#[test]
fn retained_large_catalog_label_is_completely_importable_and_exportable() {
    run(async {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/catalog-predecessor.json"
        ))
        .unwrap();
        let mut old: provider_execution::migration::CoreArchiveV3 =
            serde_json::from_str(source["archives"][1]["archive"].as_str().unwrap()).unwrap();
        old.checkpoint.skills.get_mut(&1).unwrap().name =
            "Catalog\0".repeat(MAX_SIGNABLE_BYTES / 32);
        let original = serde_json::to_vec(&old).unwrap();
        let (archive, records, reasons) =
            mock_provider::migration::from_predecessor(&original, old.checkpoint.info.root_account)
                .unwrap();
        let records = records
            .into_iter()
            .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
            .collect();
        let configuration = config(&archive);
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive.clone(), configuration)
            .await
            .unwrap();
        let current = worker.client().export_archive().unwrap().result().await;
        worker.shutdown().await.unwrap();
        let current = current.unwrap();
        assert_eq!(
            serde_json::to_vec(&current).unwrap(),
            serde_json::to_vec(&archive).unwrap()
        );
        let reasons = reasons
            .into_iter()
            .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
            .collect();
        let restored =
            mock_provider::migration::to_predecessor(current, &records, &reasons).unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), original);
    })
}

async fn retained_case(case: serde_json::Value, output: Option<&str>) {
    retained_case_with_gas(case, output, GAS).await;
}
async fn retained_case_with_gas(
    case: serde_json::Value,
    output: Option<&str>,
    gas: frame_support::weights::Weight,
) {
    let archive: CoreArchiveV4 = serde_json::from_str(case["archive"].as_str().unwrap()).unwrap();
    let mut configuration = config(&archive);
    configuration.gas_limit = gas;
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
            // This is a whole-provider workload. Ordinary per-record Calendar
            // storage needs more metered resources than the former packed blob; keep
            // the published WHOLE_GAS unchanged rather than using fixture GAS.
            retained_case_with_gas(case, None, ink_v5_compatibility::WHOLE_GAS).await;
        }
    })
}
#[test]
fn calendar_replacement_at_component_budget_preserves_state_and_reopens() {
    run(async {
        let case = reference().aggregate_cases.remove(2);
        let archive: CoreArchiveV4 =
            serde_json::from_str(case["archive"].as_str().unwrap()).unwrap();
        let configuration = config(&archive);
        assert_eq!(configuration.gas_limit, GAS);
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive.clone(), configuration.clone())
            .await
            .unwrap();
        let before = db.stored().await;
        let result = worker
            .client()
            .execute_directory_with_inputs(
                serde_json::from_value(case["signed"].clone()).unwrap(),
                serde_json::from_value(case["inputs"].clone()).unwrap(),
            )
            .unwrap()
            .result()
            .await;
        // A nested ink call can surface resource exhaustion as a caller trap.
        // The unchanged whole-provider budget executes this exact retained call
        // in the aggregate test; no failed attempt may consume history here.
        assert!(
            matches!(result, Err(Error::InvalidState | Error::ExecutionLimit(_))),
            "{result:?}"
        );
        assert_eq!(db.stored().await, before);
        worker.shutdown().await.unwrap();
        let reopened = Worker::reopen(db.0.clone(), configuration).await.unwrap();
        let retained = reopened
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(retained).unwrap(),
            serde_json::to_value(archive).unwrap()
        );
        assert_eq!(db.stored().await, before);
        reopened.shutdown().await.unwrap();
    });
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

#[test]
fn ordinary_retained_qualification_capacity_matches_native() {
    run(async {
        let source = reference().aggregate_cases.remove(2);
        let archive: CoreArchiveV4 =
            serde_json::from_str(source["archive"].as_str().unwrap()).unwrap();
        let mut configuration = config(&archive);
        configuration.gas_limit = ink_v5_compatibility::WHOLE_GAS;
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive, configuration)
            .await
            .unwrap();
        let steps: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/qualification-capacity-signed.json"
        ))
        .unwrap();
        for (index, step) in steps.into_iter().enumerate() {
            let result = worker
                .client()
                .execute_directory_with_inputs(
                    serde_json::from_value(step["signed"].clone()).unwrap(),
                    serde_json::from_value(step["inputs"].clone()).unwrap(),
                )
                .unwrap()
                .result()
                .await;
            assert_eq!(
                serde_json::to_value(result.unwrap()).unwrap(),
                step["outcome"]["receipt"],
                "step {index}"
            );
            let current = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                format!(
                    "{:x}",
                    H256::from(sha2_256(&serde_json::to_vec(&current).unwrap()))
                ),
                step["afterDigest"].as_str().unwrap(),
                "step {index}"
            );
            assert_assets(&db.stored().await, &current);
        }
        worker.shutdown().await.unwrap();
    });
}

#[test]
fn whole_resource_failure_preserves_runtime_shell_and_retry_after_restart() {
    run(async {
        let archive: CoreArchiveV4 =
            serde_json::from_str(&reference().archives[0].archive).unwrap();
        let mut configuration = config(&archive);
        configuration.gas_limit = frame_support::weights::Weight::from_parts(1, 1);
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        let signed: SignedContractCallV1 =
            serde_json::from_value(trace["steps"][0]["signed"].clone()).unwrap();
        let inputs: provider_execution::ExecutionInputs =
            serde_json::from_value(trace["steps"][0]["inputs"].clone()).unwrap();
        let db = Database::new();
        let worker = Worker::import_archive(db.0.clone(), archive.clone(), configuration.clone())
            .await
            .unwrap();
        let before = db.stored().await;
        for _ in 0..2 {
            let result = worker
                .client()
                .execute_directory_with_inputs(signed.clone(), inputs.clone())
                .unwrap()
                .result()
                .await;
            assert!(
                matches!(result, Err(Error::ExecutionLimit(_))),
                "{result:?}"
            );
            assert_eq!(before, db.stored().await);
        }
        worker.shutdown().await.unwrap();
        let restarted = Worker::reopen(db.0.clone(), configuration).await.unwrap();
        let result = restarted
            .client()
            .execute_directory_with_inputs(signed, inputs)
            .unwrap()
            .result()
            .await;
        assert!(
            matches!(result, Err(Error::ExecutionLimit(_))),
            "{result:?}"
        );
        assert_eq!(before, db.stored().await);
        let exported = restarted
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&exported).unwrap(),
            serde_json::to_vec(&archive).unwrap()
        );
        restarted.shutdown().await.unwrap();
    });
}

#[test]
fn whole_modes_serialize_primary_reference_replay_and_restart() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        use mock_provider::ProviderApi;
        let reference = reference();
        let retained = reference
            .archives
            .iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        let initial: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        for authority in [Authority::Rust, Authority::Shadow, Authority::Sandbox] {
            let db = Database::new();
            let configuration = config(&initial);
            let worker = Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let client = worker.client();
            assert_eq!(client.info().await.unwrap(), initial.checkpoint.info);
            for (index, step) in trace["steps"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .skip(70)
            {
                let call: SignedContractCallV1 =
                    serde_json::from_value(step["signed"].clone()).unwrap();
                let inputs: provider_execution::ExecutionInputs =
                    serde_json::from_value(step["inputs"].clone()).unwrap();
                let result = client
                    .execute_directory_with_inputs(call.clone(), inputs.clone())
                    .unwrap()
                    .result()
                    .await;
                if let Some(expected) = step["outcome"].get("receipt") {
                    assert_eq!(
                        serde_json::to_value(result.unwrap()).unwrap(),
                        *expected,
                        "{authority:?} receipt {index}"
                    );
                    let before = db.stored().await;
                    let replay = client
                        .execute_directory_with_inputs(call.clone(), inputs)
                        .unwrap()
                        .result()
                        .await
                        .unwrap();
                    assert_eq!(serde_json::to_value(&replay).unwrap(), *expected);
                    assert_eq!(
                        before,
                        db.stored().await,
                        "replay must not advance revision"
                    );
                    assert_eq!(
                        client.receipt(call.call.operation_id).await.unwrap(),
                        Some(replay)
                    );
                } else {
                    assert!(result.is_err(), "{authority:?} infrastructure {index}");
                }
                let archive = client.export_archive().unwrap().result().await.unwrap();
                assert_eq!(
                    format!(
                        "{:x}",
                        H256::from(sha2_256(&serde_json::to_vec(&archive).unwrap()))
                    ),
                    reference.digests[index],
                    "{authority:?} archive {index}"
                );
                let diagnostics = client.whole_diagnostics().unwrap().result().await.unwrap();
                assert_eq!(
                    diagnostics.comparison,
                    if authority == Authority::Rust {
                        "disabled"
                    } else {
                        "matched"
                    },
                    "{authority:?} comparison {index}"
                );
            }
            let before = client.export_archive().unwrap().result().await.unwrap();
            let snapshot = client.snapshot().await.unwrap();
            assert_eq!(
                snapshot,
                mock_provider::ReferenceState::from_archive(configuration.root, before.clone())
                    .unwrap()
                    .snapshot()
            );
            worker.shutdown().await.unwrap();
            let worker = Worker::reopen_whole(db.0.clone(), configuration.clone(), authority)
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_vec(
                    &worker
                        .client()
                        .export_archive()
                        .unwrap()
                        .result()
                        .await
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_vec(&before).unwrap()
            );
            worker.shutdown().await.unwrap();
            for wrong in [Authority::Rust, Authority::Shadow, Authority::Sandbox]
                .into_iter()
                .filter(|a| *a != authority)
            {
                assert!(
                    Worker::reopen_whole(db.0.clone(), configuration.clone(), wrong)
                        .await
                        .is_err()
                );
            }
        }
    });
}

async fn whole_rows(db: &Database) -> ((i64, Vec<u8>, Vec<u8>), (Vec<u8>, String, i64)) {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.0.display()))
        .await
        .unwrap();
    let reference =
        sqlx::query_as("SELECT archive,comparison,comparisons FROM candidate_reference")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;
    (db.stored().await, reference)
}
#[test]
fn whole_reference_storage_failure_competition_and_incomplete_shadow_are_explicit() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        let retained = reference()
            .archives
            .into_iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        let initial: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        let call: SignedContractCallV1 =
            serde_json::from_value(trace["steps"][70]["signed"].clone()).unwrap();
        let inputs: provider_execution::ExecutionInputs =
            serde_json::from_value(trace["steps"][70]["inputs"].clone()).unwrap();
        for authority in [Authority::Rust, Authority::Shadow, Authority::Sandbox] {
            let db = Database::new();
            let configuration = config(&initial);
            let worker = Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let client = worker.client();
            let before = whole_rows(&db).await;
            let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.0.display()))
                .await
                .unwrap();
            sqlx::query("CREATE TRIGGER fail_reference AFTER UPDATE ON candidate_reference BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(&pool).await.unwrap();
            assert!(matches!(
                client
                    .execute_directory_with_inputs(call.clone(), inputs.clone())
                    .unwrap()
                    .result()
                    .await,
                Err(Error::Storage(_))
            ));
            assert_eq!(
                whole_rows(&db).await,
                before,
                "{authority:?}: reference failure rolls back both engines"
            );
            sqlx::query("DROP TRIGGER fail_reference")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
            let other = Worker::reopen_whole(db.0.clone(), configuration, authority)
                .await
                .unwrap();
            let left = client
                .execute_directory_with_inputs(call.clone(), inputs.clone())
                .unwrap();
            let right = other
                .client()
                .execute_directory_with_inputs(call.clone(), inputs.clone())
                .unwrap();
            assert_eq!(left.result().await.unwrap(), right.result().await.unwrap());
            assert_eq!(whole_rows(&db).await.0.0, before.0.0 + 1);
            other.shutdown().await.unwrap();
            worker.shutdown().await.unwrap();
        }
        for authority in [Authority::Shadow, Authority::Sandbox] {
            let db = Database::new();
            let mut configuration = config(&initial);
            configuration.gas_limit = frame_support::weights::Weight::from_parts(1, 1);
            let worker = Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let before = whole_rows(&db).await;
            let result = worker
                .client()
                .execute_directory_with_inputs(call.clone(), inputs.clone())
                .unwrap()
                .result()
                .await;
            if authority == Authority::Sandbox {
                assert!(matches!(result, Err(Error::ExecutionLimit(_))));
                assert_eq!(
                    whole_rows(&db).await,
                    before,
                    "primary failure commits nothing"
                );
            } else {
                assert_eq!(
                    serde_json::to_value(result.unwrap()).unwrap(),
                    trace["steps"][70]["outcome"]["receipt"]
                );
                assert_eq!(
                    worker
                        .client()
                        .whole_diagnostics()
                        .unwrap()
                        .result()
                        .await
                        .unwrap()
                        .comparison,
                    "incomplete"
                );
            }
            worker.shutdown().await.unwrap();
            let worker = Worker::reopen_whole(db.0.clone(), configuration, authority)
                .await
                .unwrap();
            if authority == Authority::Shadow {
                let next = &trace["steps"][71];
                worker
                    .client()
                    .execute_directory_with_inputs(
                        serde_json::from_value(next["signed"].clone()).unwrap(),
                        serde_json::from_value(next["inputs"].clone()).unwrap(),
                    )
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
                let diagnostics = worker
                    .client()
                    .whole_diagnostics()
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
                assert_eq!(diagnostics.comparison, "incomplete");
                assert_eq!(diagnostics.comparisons, 1);
            } else {
                assert_eq!(whole_rows(&db).await, before);
            }
            worker.shutdown().await.unwrap();
        }
    });
}

#[test]
fn whole_startup_rejects_legacy_storage_and_invalid_envelopes_without_changes() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        let retained = reference()
            .archives
            .into_iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        let initial: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
        let configuration = config(&initial);
        let unsupported = Database::new();
        let mut disabled = configuration.clone();
        disabled.mock_funding = false;
        assert!(
            Worker::import_whole(
                unsupported.0.clone(),
                initial.clone(),
                disabled,
                Authority::Rust
            )
            .await
            .is_err()
        );
        assert!(!unsupported.0.exists());
        let db = Database::new();
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.0.display()))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE provider_state (singleton INTEGER, state BLOB)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO provider_state VALUES (1, X'010203')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                Authority::Rust
            )
            .await
            .is_err()
        );
        let tables: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tables, 1);
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT state FROM provider_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(bytes, [1, 2, 3]);
        pool.close().await;
        let unrelated = Database::new();
        let pool =
            sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", unrelated.0.display()))
                .await
                .unwrap();
        sqlx::query("CREATE TABLE sqliteXforeign (retained BLOB)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            Worker::import_whole(
                unrelated.0.clone(),
                initial.clone(),
                configuration.clone(),
                Authority::Rust
            )
            .await
            .is_err()
        );
        let tables: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='table'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tables, 1);
        pool.close().await;
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        let call: SignedContractCallV1 =
            serde_json::from_value(trace["steps"][70]["signed"].clone()).unwrap();
        for authority in [Authority::Rust, Authority::Shadow, Authority::Sandbox] {
            let db = Database::new();
            let worker = Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let before = whole_rows(&db).await;
            let mut invalid = call.clone();
            invalid.signature = Sr25519Signature::from_bytes([0; 64]);
            assert!(
                matches!(worker.client().execute_signed(invalid, UnixSeconds::new(1)).unwrap().result().await, Err(Error::Shell(error)) if error.code() == "invalid_signature")
            );
            assert!(
                matches!(worker.client().execute_signed(call.clone(), UnixSeconds::new(20_000)).unwrap().result().await, Err(Error::Shell(error)) if error.code() == "expired_call")
            );
            let native =
                mock_provider::Provider::memory_from_archive(configuration.root, initial.clone())
                    .await
                    .unwrap();
            let mut oversized = call.clone();
            oversized.call.command = ProviderCommand::CreateProject(CreateProjectRequest {
                title: "Oversized transport call".into(),
                description: "x".repeat(ink_v5_compatibility::worker::MAX_INPUT + 1),
            });
            let expected = mock_provider::ProviderApi::execute(
                &native,
                oversized.clone(),
                UnixSeconds::new(1),
            )
            .await
            .unwrap_err();
            let actual = mock_provider::ProviderApi::execute(
                &worker.client(),
                oversized,
                UnixSeconds::new(1),
            )
            .await
            .unwrap_err();
            assert_eq!(
                actual, expected,
                "oversized envelopes preserve native HTTP status and code"
            );
            assert_eq!(before, whole_rows(&db).await);
            worker.shutdown().await.unwrap();
        }
    });
}

#[test]
fn whole_differences_persist_without_substituting_either_primary() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        let retained = reference()
            .archives
            .into_iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        let initial: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        for authority in [Authority::Shadow, Authority::Sandbox] {
            let db = Database::new();
            let configuration = config(&initial);
            let worker = Worker::import_whole(
                db.0.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let mut changed = initial.clone();
            changed
                .checkpoint
                .skill_metadata
                .values_mut()
                .next()
                .unwrap()
                .origin = generated_contracts::provider_core::CatalogReferenceV1::Migration(
                PayloadHash::from_bytes([99; 32]),
            );
            let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.0.display()))
                .await
                .unwrap();
            sqlx::query("UPDATE candidate_reference SET archive=?")
                .bind(serde_json::to_vec(&changed).unwrap())
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
            let mut expected = mock_provider::ReferenceState::from_archive(
                configuration.root,
                if authority == Authority::Shadow {
                    changed
                } else {
                    initial.clone()
                },
            )
            .unwrap();
            let step = &trace["steps"][70];
            let call: SignedContractCallV1 =
                serde_json::from_value(step["signed"].clone()).unwrap();
            let inputs: provider_execution::ExecutionInputs =
                serde_json::from_value(step["inputs"].clone()).unwrap();
            let (next, receipt) = expected
                .execute(
                    provider_execution::VerifiedCall::verify(call.clone()).unwrap(),
                    inputs.clone(),
                )
                .unwrap();
            expected = next;
            assert_eq!(
                worker
                    .client()
                    .execute_directory_with_inputs(call, inputs)
                    .unwrap()
                    .result()
                    .await
                    .unwrap(),
                receipt
            );
            assert_eq!(
                worker
                    .client()
                    .whole_diagnostics()
                    .unwrap()
                    .result()
                    .await
                    .unwrap()
                    .comparison,
                "different"
            );
            worker.shutdown().await.unwrap();
            let worker = Worker::reopen_whole(db.0.clone(), configuration, authority)
                .await
                .unwrap();
            let step = &trace["steps"][71];
            let call: SignedContractCallV1 =
                serde_json::from_value(step["signed"].clone()).unwrap();
            let inputs: provider_execution::ExecutionInputs =
                serde_json::from_value(step["inputs"].clone()).unwrap();
            let (expected, receipt) = expected
                .execute(
                    provider_execution::VerifiedCall::verify(call.clone()).unwrap(),
                    inputs.clone(),
                )
                .unwrap();
            assert_eq!(
                worker
                    .client()
                    .execute_directory_with_inputs(call, inputs)
                    .unwrap()
                    .result()
                    .await
                    .unwrap(),
                receipt
            );
            let diagnostics = worker
                .client()
                .whole_diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(diagnostics.comparison, "different");
            assert_eq!(diagnostics.comparisons, 1);
            assert_eq!(
                serde_json::to_vec(
                    &worker
                        .client()
                        .export_archive()
                        .unwrap()
                        .result()
                        .await
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_vec(&expected.archive().unwrap()).unwrap()
            );
            worker.shutdown().await.unwrap();
        }
    });
}

#[test]
fn evidence_commitments_match_native_with_legacy_history_and_lossless_rollback() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        let initial: CoreArchiveV4 = serde_json::from_str(
            &reference()
                .archives
                .into_iter()
                .find(|a| a.before_step == 70)
                .unwrap()
                .archive,
        )
        .unwrap();
        let configuration = config(&initial);
        let trace: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
        ))
        .unwrap();
        let overlay: serde_json::Value = serde_json::from_str(include_str!(
            "../../../services/mock-provider/src/domain/fixtures/evidence-signed.json"
        ))
        .unwrap();
        let db = Database::new();
        let mut worker = Worker::import_whole(
            db.0.clone(),
            initial.clone(),
            configuration.clone(),
            Authority::Sandbox,
        )
        .await
        .unwrap();
        let mut expected = initial;
        for (index, step) in trace["steps"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .skip(70)
        {
            let replacement = overlay["replacements"]
                .as_array()
                .unwrap()
                .iter()
                .find(|value| value["step"] == index);
            let call: SignedContractCallV1 = serde_json::from_value(
                replacement.map_or_else(|| step["signed"].clone(), |value| value["signed"].clone()),
            )
            .unwrap();
            let inputs: provider_execution::ExecutionInputs =
                serde_json::from_value(step["inputs"].clone()).unwrap();
            let native = provider_execution::VerifiedCall::verify(call.clone()).map(|verified| {
                mock_provider::ReferenceState::from_archive(configuration.root, expected.clone())
                    .unwrap()
                    .execute(verified, inputs.clone())
            });
            let before = db.stored().await;
            let result = worker
                .client()
                .execute_directory_with_inputs(call.clone(), inputs.clone())
                .unwrap()
                .result()
                .await;
            match native {
                Ok(Ok((state, receipt))) => {
                    assert_eq!(result.unwrap(), receipt, "receipt {index}");
                    expected = state.archive().unwrap();
                    let current = db.stored().await;
                    assert_eq!(
                        worker
                            .client()
                            .execute_directory_with_inputs(call, inputs)
                            .unwrap()
                            .result()
                            .await
                            .unwrap(),
                        receipt
                    );
                    assert_eq!(db.stored().await, current, "replay {index}");
                }
                _ => {
                    assert!(result.is_err(), "infrastructure {index}");
                    assert_eq!(db.stored().await, before, "failure {index}");
                }
            }
            let archive = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_vec(&archive).unwrap(),
                serde_json::to_vec(&expected).unwrap(),
                "complete archive {index}"
            );
            assert_eq!(
                worker
                    .client()
                    .whole_diagnostics()
                    .unwrap()
                    .result()
                    .await
                    .unwrap()
                    .comparison,
                "matched"
            );
            if index == 104 {
                worker.shutdown().await.unwrap();
                worker =
                    Worker::reopen_whole(db.0.clone(), configuration.clone(), Authority::Sandbox)
                        .await
                        .unwrap();
                let fresh = Database::new();
                let imported =
                    Worker::import_archive(fresh.0.clone(), archive.clone(), configuration.clone())
                        .await
                        .unwrap();
                assert_eq!(
                    serde_json::to_vec(
                        &imported
                            .client()
                            .export_archive()
                            .unwrap()
                            .result()
                            .await
                            .unwrap()
                    )
                    .unwrap(),
                    serde_json::to_vec(&archive).unwrap()
                );
                // A native rollback can immediately consume this complete export.
                assert_eq!(
                    serde_json::to_vec(
                        &mock_provider::ReferenceState::from_archive(configuration.root, archive)
                            .unwrap()
                            .archive()
                            .unwrap()
                    )
                    .unwrap(),
                    serde_json::to_vec(&expected).unwrap()
                );
                imported.shutdown().await.unwrap();
            }
        }
        let before = db.stored().await;
        worker.shutdown().await.unwrap();
        // Full old preimages live only in the SDK checkpoint. Losing one makes
        // startup fail instead of silently losing historical evidence.
        let mut shell: serde_json::Value = serde_json::from_slice(&before.2).unwrap();
        assert!(!shell["legacy_references"].as_object().unwrap().is_empty());
        shell["legacy_references"] = serde_json::json!({});
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.0.display()))
            .await
            .unwrap();
        sqlx::query("UPDATE candidate_state SET shell=$1")
            .bind(serde_json::to_vec(&shell).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert!(
            Worker::reopen_whole(db.0.clone(), configuration, Authority::Sandbox)
                .await
                .is_err()
        );
    });
}

#[test]
fn task_state_exports_actual_selected_counter_and_preserves_exhaustion() {
    run(async {
        use ink_v5_compatibility::worker::Authority;
        use mock_provider::ProviderApi;
        let reference = reference();
        let retained = reference
            .archives
            .iter()
            .find(|a| a.before_step == 70)
            .unwrap();
        for counter in [99, u32::MAX] {
            let mut archive: CoreArchiveV4 = serde_json::from_str(&retained.archive).unwrap();
            archive.checkpoint.next_task_id = counter;
            let configuration = config(&archive);
            let expected =
                mock_provider::ReferenceState::from_archive(configuration.root, archive.clone())
                    .unwrap()
                    .task_state();
            expected.validate().unwrap();
            assert_eq!(expected.next_task_id, counter);
            assert!(expected.storages.iter().any(|r| r.storage.tasks.is_empty()));
            for authority in [Authority::Rust, Authority::Shadow, Authority::Sandbox] {
                let db = Database::new();
                let worker = Worker::import_whole(
                    db.0.clone(),
                    archive.clone(),
                    configuration.clone(),
                    authority,
                )
                .await
                .unwrap();
                let before = db.stored().await;
                let observed = worker.client().task_state().await.unwrap();
                assert_eq!(observed, expected, "{authority:?} counter {counter}");
                assert_eq!(db.stored().await, before, "read must not commit");
                worker.shutdown().await.unwrap();
            }
        }
    });
}
