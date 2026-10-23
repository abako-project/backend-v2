//! Real compact creation, explicit predecessor exchange, replay and restart.
use generated_contracts::{provider_core::*, *};
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use parity_scale_codec::Encode;
use provider_execution::{
    ExecutionInputs,
    migration::{ArchiveV1, CoreArchiveV2},
};
use sp_core::{Pair, sr25519};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn signed(
    core: &CoreArchiveV2,
    actor: u8,
    op: u8,
    command: ProviderCommand,
) -> SignedContractCallV1 {
    let origin = account(actor);
    let call = UnsignedContractCallV1::new(
        core.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([op; 16]),
        origin,
        *core.checkpoint.nonces.get(&origin).unwrap_or(&0),
        UnixSeconds::new(u64::MAX),
        command,
    )
    .unwrap();
    let signature = Sr25519Signature::from_bytes(
        sr25519::Pair::from_seed(&[actor; 32])
            .sign(&call.signable_bytes().unwrap())
            .0,
    );
    SignedContractCallV1 { call, signature }
}
fn commitment(record: &CreationMetadataRecordV1, op: u8) -> MetadataCommitmentV1 {
    MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([op; 16]),
        hash: mock_provider::migration::source_hash(&record.hash_input()),
    }
}
fn config(core: &CoreArchiveV2) -> WholeConfiguration {
    WholeConfiguration {
        root: core.checkpoint.info.root_account,
        generation: core.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    }
}
fn private_file(path: &Path) -> File {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap()
}
fn json_file(path: &Path, value: &impl serde::Serialize) {
    let mut file = private_file(path);
    serde_json::to_writer(&mut file, value).unwrap();
    file.sync_all().unwrap();
}
struct Rehearsal {
    core: CoreArchiveV2,
    old: ArchiveV1,
    records: BTreeMap<PayloadHash, CreationMetadataRecordV1>,
    calls: Vec<SignedContractCallV1>,
    receipts: Vec<OperationReceipt>,
    next: Vec<SignedContractCallV1>,
}
fn export(case: &Rehearsal, directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    json_file(&directory.join("core.json"), &case.core);
    json_file(&directory.join("predecessor.json"), &case.old);
    json_file(&directory.join("calls.json"), &case.calls);
    json_file(&directory.join("receipts.json"), &case.receipts);
    json_file(&directory.join("next.json"), &case.next);
    let mut file = private_file(&directory.join("metadata.ndjson"));
    serde_json::to_writer(
        &mut file,
        &CreationMetadataHeaderV1 {
            schema_version: 1,
            generation: case.core.checkpoint.info.provider_instance_id,
        },
    )
    .unwrap();
    file.write_all(b"\n").unwrap();
    for record in case.records.values() {
        serde_json::to_writer(&mut file, record).unwrap();
        file.write_all(b"\n").unwrap();
    }
    file.sync_all().unwrap();
    let mut file = private_file(&directory.join("root-account.hex"));
    write!(&mut file, "{}", case.core.checkpoint.info.root_account).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn compact_success_migrates_replays_recovers_and_rehydrates_current_predecessor_state() {
    let directory = std::env::temp_dir().join(format!(
        "ink-core-metadata-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("current.sqlite");
    let case = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let predecessor: serde_json::Value = serde_json::from_str(include_str!(
                "../../../services/mock-provider/src/domain/fixtures/predecessor-migration.json"
            ))
            .unwrap();
            let source = predecessor["archives"][1]["archive"]
                .as_str()
                .unwrap()
                .as_bytes();
            let (initial, migrated) =
                mock_provider::migration::from_predecessor(source, account(1)).unwrap();
            let mut records: BTreeMap<_, _> = migrated
                .into_iter()
                .map(|record| {
                    (
                        mock_provider::migration::source_hash(&record.hash_input()),
                        record,
                    )
                })
                .collect();
            let mut worker = Worker::import_whole(
                path.clone(),
                initial.clone(),
                config(&initial),
                Authority::Sandbox,
            )
            .await
            .unwrap();
            let mut current = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_vec(&current).unwrap(),
                serde_json::to_vec(&initial).unwrap()
            );
            let generation = current.checkpoint.info.provider_instance_id;
            let name = format!("Compact worker\0{}", "名".repeat(8192));
            let record = CreationMetadataRecordV1 {
                generation,
                provenance: MetadataProvenanceV1::Creation {
                    operation_id: OperationId::from_bytes([241; 16]),
                    author: account(8),
                },
                content: CreationContentV1::WorkerName {
                    display_name: name.clone(),
                },
            };
            let command = ProviderCommand::RegisterWorkerCore(RegisterWorkerCoreRequest {
                content: commitment(&record, 241),
                qualifications: Qualifications {
                    role_ids: vec![3],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(2400),
                    overrides: vec![],
                },
            });
            records.insert(
                mock_provider::migration::source_hash(&record.hash_input()),
                record,
            );
            let first = signed(&current, 8, 241, command);
            let mut calls = Vec::new();
            let mut receipts = Vec::new();
            let receipt = worker
                .client()
                .execute_directory_with_inputs(
                    first.clone(),
                    ExecutionInputs {
                        version: 1,
                        now: UnixSeconds::new(500),
                        seed: [241; 32],
                    },
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(receipt.outcome, ExecutionOutcome::Success);
            calls.push(first);
            receipts.push(receipt);
            current = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            let core_worker = &current.checkpoint.workers[&account(8)];
            assert!(core_worker.encoded_size() < 16 * 1024);
            assert!(
                serde_json::to_value(core_worker)
                    .unwrap()
                    .get("displayName")
                    .is_none()
            );
            let title = "Immutable\0Project".to_owned();
            let description = "p".repeat(200_000);
            let record = CreationMetadataRecordV1 {
                generation,
                provenance: MetadataProvenanceV1::Creation {
                    operation_id: OperationId::from_bytes([242; 16]),
                    author: account(9),
                },
                content: CreationContentV1::ProjectText {
                    title: title.clone(),
                    description: description.clone(),
                },
            };
            let command = ProviderCommand::CreateProjectCore(commitment(&record, 242));
            records.insert(
                mock_provider::migration::source_hash(&record.hash_input()),
                record,
            );
            let second = signed(&current, 9, 242, command);
            let receipt = worker
                .client()
                .execute_directory_with_inputs(
                    second.clone(),
                    ExecutionInputs {
                        version: 1,
                        now: UnixSeconds::new(501),
                        seed: [242; 32],
                    },
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(receipt.outcome, ExecutionOutcome::Success);
            let project = receipt.created_entity_id.unwrap();
            calls.push(second);
            receipts.push(receipt);
            current = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert!(current.checkpoint.projects[&project].encoded_size() < 16 * 1024);
            assert!(
                serde_json::to_value(&current.checkpoint.projects[&project])
                    .unwrap()
                    .get("description")
                    .is_none()
            );
            // Direct legacy calls retain their original bytes, but use the one core
            // handler. Their immutable preimages can be recovered from success history.
            let legacy = ProviderCommand::RegisterWorker(RegisterWorkerRequest {
                display_name: "Recovered legacy\0worker".into(),
                qualifications: Qualifications {
                    role_ids: vec![3],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(600),
                    overrides: vec![],
                },
            });
            for (op, expected) in [
                (243, ExecutionOutcome::Success),
                (244, ExecutionOutcome::Failed("worker_exists".into())),
            ] {
                let call = signed(&current, 10, op, legacy.clone());
                let receipt = worker
                    .client()
                    .execute_directory_with_inputs(
                        call.clone(),
                        ExecutionInputs {
                            version: 1,
                            now: UnixSeconds::new(502),
                            seed: [op; 32],
                        },
                    )
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
                assert_eq!(receipt.outcome, expected);
                calls.push(call);
                receipts.push(receipt);
                current = worker
                    .client()
                    .export_archive()
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
            }
            assert!(mock_provider::migration::to_predecessor(current.clone(), &records).is_err());
            let recovered = mock_provider::migration::recover_legacy_creations(&current).unwrap();
            assert_eq!(recovered.len(), 1);
            for record in recovered {
                records.insert(
                    mock_provider::migration::source_hash(&record.hash_input()),
                    record,
                );
            }
            let old = mock_provider::migration::to_predecessor(current.clone(), &records).unwrap();
            assert_eq!(old.checkpoint.workers[&account(8)].display_name, name);
            assert_eq!(old.checkpoint.projects[&project].title, title);
            assert_eq!(old.checkpoint.projects[&project].description, description);
            assert_eq!(
                serde_json::to_vec(&old.checkpoint.receipts).unwrap(),
                serde_json::to_vec(&current.checkpoint.receipts).unwrap()
            );
            assert_eq!(old.checkpoint.nonces, current.checkpoint.nonces);
            assert_eq!(old.checkpoint.events, current.checkpoint.events);
            assert_eq!(old.checkpoint.balances, initial.checkpoint.balances);
            assert_eq!(old.checkpoint.next_task_id, initial.checkpoint.next_task_id);
            let mut tampered = records.clone();
            let key = current.checkpoint.workers[&account(8)].content.hash();
            tampered.get_mut(&key).unwrap().content = CreationContentV1::WorkerName {
                display_name: "substituted".into(),
            };
            assert!(mock_provider::migration::to_predecessor(current.clone(), &tampered).is_err());
            let mut swapped = current.clone();
            swapped
                .checkpoint
                .projects
                .get_mut(&project)
                .unwrap()
                .content = current.checkpoint.workers[&account(8)].content.clone();
            assert!(mock_provider::ReferenceState::from_archive(account(1), swapped).is_err());
            let next = vec![
                signed(
                    &current,
                    1,
                    245,
                    ProviderCommand::SetScorePolicy(
                        ScorePolicy::new(
                            Percentage::new(50).unwrap(),
                            Percentage::new(50).unwrap(),
                        )
                        .unwrap(),
                    ),
                ),
                signed(
                    &current,
                    9,
                    246,
                    ProviderCommand::CreateProject(CreateProjectRequest {
                        title: "Post-rollback project".into(),
                        description: "Ordinary predecessor work".into(),
                    }),
                ),
            ];
            worker.shutdown().await.unwrap();
            worker = Worker::reopen_whole(path.clone(), config(&current), Authority::Sandbox)
                .await
                .unwrap();
            for (call, receipt) in calls.iter().zip(&receipts) {
                let replay = worker
                    .client()
                    .execute_directory_with_inputs(
                        call.clone(),
                        ExecutionInputs {
                            version: 99,
                            now: UnixSeconds::new(u64::MAX),
                            seed: [0; 32],
                        },
                    )
                    .unwrap()
                    .result()
                    .await
                    .unwrap();
                assert_eq!(replay, *receipt);
            }
            let after = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_vec(&after).unwrap(),
                serde_json::to_vec(&current).unwrap()
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
            worker.shutdown().await.unwrap();
            Rehearsal {
                core: current,
                old,
                records,
                calls,
                receipts,
                next,
            }
        });
    if let Some(output) = std::env::var_os("R2A_ROLLBACK_OUTPUT_DIR") {
        export(&case, &PathBuf::from(output));
    }
    std::fs::remove_dir_all(directory).unwrap();
}
