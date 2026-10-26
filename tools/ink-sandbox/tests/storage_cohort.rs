//! Explicit current-state handoff to a frozen preceding executable, not a rebuild
//! of the same source under a different authority flag.
use generated_contracts::*;
use ink_sandbox::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::ReferenceState;
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV5};
use serde_json::Value;
use sp_core::{Pair, hashing::sha2_256, sr25519};
use std::{fs, path::Path, process::Command};

fn cli(binary: &Path, database: &Path, root: &Path, authority: &str, command: &str, file: &Path) {
    let output = Command::new(binary)
        .arg(command)
        .arg(file)
        .env("WHOLE_DATABASE_PATH", database)
        .env("MOCK_ROOT_ACCOUNT_FILE", root)
        .env("PROVIDER_AUTHORITY", authority)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
#[ignore = "requires INK_STORAGE_PREDECESSOR and its recorded INK_STORAGE_PREDECESSOR_SHA256"]
fn current_directory_project_and_calendar_writes_restore_into_frozen_predecessor() {
    let predecessor =
        std::env::var_os("INK_STORAGE_PREDECESSOR").expect("frozen predecessor required");
    let predecessor = Path::new(&predecessor);
    let hash: String = sha2_256(&fs::read(predecessor).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash,
        std::env::var("INK_STORAGE_PREDECESSOR_SHA256").unwrap()
    );
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed-v5.json"
    ))
    .unwrap();
    let initial: CoreArchiveV5 = serde_json::from_str(
        fixtures["archives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["beforeStep"] == 70)
            .unwrap()["archive"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let (origin, retained) = initial
        .checkpoint
        .workers
        .iter()
        .find(|(_, w)| !w.calendar.reservations.is_empty())
        .unwrap();
    let key = (1..=10)
        .map(|seed| sr25519::Pair::from_seed(&[seed; 32]))
        .find(|key| key.public().0 == *origin.as_bytes())
        .unwrap();
    let mut definition = retained.calendar.definition.clone();
    definition.default_weekly_minutes = Minutes::new(10_000);
    definition.overrides = (2028..2068)
        .flat_map(|year| {
            (1..=50).map(move |week| WeekOverride {
                week: Week::new(year, week).unwrap(),
                capacity: Minutes::new(10_000),
            })
        })
        .collect();
    let command = ProviderCommand::SetCalendar(definition);
    let mut qualifications = retained.qualifications.clone();
    if let Some(role) = initial
        .checkpoint
        .roles
        .keys()
        .find(|role| !qualifications.role_ids.contains(role))
    {
        qualifications.role_ids.push(*role);
    } else {
        qualifications.role_ids.reverse();
    }
    assert_ne!(
        qualifications, retained.qualifications,
        "fixture must change actual Directory facts"
    );
    let configuration = WholeConfiguration {
        root: initial.checkpoint.info.root_account,
        generation: initial.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    };
    let base = std::env::temp_dir().join(format!("directory-cohort-{}", std::process::id()));
    fs::create_dir(&base).unwrap();
    let root = base.join("root.hex");
    fs::write(&root, configuration.root.to_string()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (name, authority) in [
        ("rust", Authority::Rust),
        ("shadow", Authority::Shadow),
        ("sandbox", Authority::Sandbox),
    ] {
        let directory = base.join(name);
        fs::create_dir(&directory).unwrap();
        let database = directory.join("current.sqlite");
        let qualifier = UnsignedContractCallV1::new(
            configuration.generation,
            OperationId::from_bytes([244; 16]),
            *origin,
            *initial.checkpoint.nonces.get(origin).unwrap_or(&0),
            UnixSeconds::new(1_900_000_000),
            ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
                qualifications: qualifications.clone(),
            }),
        )
        .unwrap();
        let qualifier = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(
                key.sign(&qualifier.signable_bytes().unwrap()).0,
            ),
            call: qualifier,
        };
        let call = UnsignedContractCallV1::new(
            configuration.generation,
            OperationId::from_bytes([245; 16]),
            *origin,
            initial
                .checkpoint
                .nonces
                .get(origin)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .unwrap(),
            UnixSeconds::new(1_900_000_000),
            command.clone(),
        )
        .unwrap();
        let signature = Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0);
        let signed = SignedContractCallV1 { call, signature };
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(1_800_000_000),
            seed: [240; 32],
        };
        let reference = ReferenceState::from_archive(configuration.root, initial.clone()).unwrap();
        let (reference, qualified) = reference
            .execute(
                VerifiedCall::verify(qualifier.clone()).unwrap(),
                inputs.clone(),
            )
            .unwrap();
        assert_eq!(qualified.outcome, ExecutionOutcome::Success);
        let (reference, expected) = reference
            .execute(
                VerifiedCall::verify(signed.clone()).unwrap(),
                inputs.clone(),
            )
            .unwrap();
        assert_eq!(expected.outcome, ExecutionOutcome::Success);
        let project = initial
            .checkpoint
            .projects
            .values()
            .find(|p| p.proposals[0].status == ProposalStatus::PendingApproval)
            .unwrap();
        let project_id = project.project_id;
        let project_origin = project.coordinator;
        let storage_id = project.proposals[0].milestones[0]
            .task_storage
            .task_storage_id;
        let task_id = initial.checkpoint.next_task_id;
        let project_key = (1..=10)
            .map(|seed| sr25519::Pair::from_seed(&[seed; 32]))
            .find(|key| AccountId32::from_bytes(key.public().0) == project_origin)
            .unwrap();
        let task_call = UnsignedContractCallV1::new(
            configuration.generation,
            OperationId::from_bytes([243; 16]),
            project_origin,
            reference.nonce(project_origin).nonce,
            UnixSeconds::new(1_900_000_000),
            ProviderCommand::CreateTaskCore {
                project_id,
                task_storage_id: storage_id,
                assignees: vec![*origin],
            },
        )
        .unwrap();
        let task_call = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(
                project_key.sign(&task_call.signable_bytes().unwrap()).0,
            ),
            call: task_call,
        };
        let (reference, task_expected) = reference
            .execute(
                VerifiedCall::verify(task_call.clone()).unwrap(),
                inputs.clone(),
            )
            .unwrap();
        assert_eq!(task_expected.outcome, ExecutionOutcome::Success);
        runtime.block_on(async {
            let worker = Worker::import_whole(
                database.clone(),
                initial.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            let qualified_receipt = worker
                .client()
                .execute_directory_with_inputs(qualifier.clone(), inputs.clone())
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(qualified_receipt, qualified);
            let receipt = worker
                .client()
                .execute_directory_with_inputs(signed.clone(), inputs.clone())
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(receipt, expected);
            let task_receipt = worker
                .client()
                .execute_directory_with_inputs(task_call.clone(), inputs)
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(task_receipt, task_expected);
            let diagnostics = worker
                .client()
                .whole_diagnostics()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                diagnostics.comparison,
                if authority == Authority::Rust {
                    "disabled"
                } else {
                    "matched"
                }
            );
            assert_eq!(
                diagnostics.comparisons,
                if authority == Authority::Rust { 0 } else { 3 }
            );
            worker.shutdown().await.unwrap();
        });
        // Both CLI exports reopen the actual runtime, including Sandbox authority.
        let current = directory.join("current.json");
        cli(
            Path::new(env!("CARGO_BIN_EXE_whole-provider")),
            &database,
            &root,
            name,
            "export",
            &current,
        );
        assert_eq!(
            read(&current),
            serde_json::to_value(reference.archive().unwrap()).unwrap()
        );
        let predecessor_archive = directory.join("current-v5.json");
        fs::write(
            &predecessor_archive,
            serde_json::to_vec(&reference.archive().unwrap()).unwrap(),
        )
        .unwrap();
        let restored = directory.join("preceding.sqlite");
        cli(
            predecessor,
            &restored,
            &root,
            name,
            "import",
            &predecessor_archive,
        );
        // Bundle cleanup changes identity even when business WASM is unchanged.
        // A fresh import is required; never reinterpret the old checkpoint.
        runtime.block_on(async {
            assert!(
                Worker::reopen_whole(restored.clone(), configuration.clone(), authority)
                    .await
                    .is_err()
            );
        });
        for phase in ["first", "reopened"] {
            let exported = directory.join(format!("preceding-{phase}.json"));
            cli(predecessor, &restored, &root, name, "export", &exported);
            assert_eq!(
                read(&exported),
                read(&predecessor_archive),
                "{name}/{phase}: complete state/history changed"
            );
        }
        let mut next = task_call.call.clone();
        next.operation_id = OperationId::from_bytes([246; 16]);
        next.nonce = reference.nonce(project_origin).nonce;
        next.command = ProviderCommand::SetTaskAssigneesCore {
            project_id,
            task_storage_id: storage_id,
            task_id,
            assignees: Vec::new(),
        };
        let fresh = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(
                project_key.sign(&next.signable_bytes().unwrap()).0,
            ),
            call: next,
        };
        fs::write(
            directory.join("calls.json"),
            serde_json::to_vec(&[&signed, &fresh]).unwrap(),
        )
        .unwrap();
        let exercised = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/storage_cohort_http.py"
            ))
            .arg(predecessor)
            .arg(&restored)
            .arg(&root)
            .arg(name)
            .arg(&directory)
            .output()
            .unwrap();
        assert!(
            exercised.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&exercised.stderr)
        );
        let receipts: Vec<OperationReceipt> =
            serde_json::from_slice(&fs::read(directory.join("receipts.json")).unwrap()).unwrap();
        assert_eq!(receipts[0], expected, "retained signed replay");
        assert_eq!(receipts[1].outcome, ExecutionOutcome::Success);
        assert_eq!(receipts[1], receipts[2]);
        let after_path = directory.join("after-work.json");
        cli(predecessor, &restored, &root, name, "export", &after_path);
        let after: CoreArchiveV5 = serde_json::from_value(read(&after_path)).unwrap();
        let captured = after.checkpoint.receipts[&fresh.call.operation_id]
            .inputs
            .clone()
            .unwrap();
        let (reference, fresh_expected) = reference
            .execute(VerifiedCall::verify(fresh).unwrap(), captured)
            .unwrap();
        assert_eq!(receipts[1], fresh_expected);
        assert_eq!(
            read(&after_path),
            serde_json::to_value(reference.archive().unwrap()).unwrap(),
            "{name}: replay/further work changed unrelated state or history"
        );
        // Roll forward from the predecessor's latest writes through the logical
        // archive, into a fresh current store, then verify reopen and signed replay.
        let forward = directory.join("forward.sqlite");
        runtime.block_on(async {
            let worker = Worker::import_whole(
                forward.clone(),
                after.clone(),
                configuration.clone(),
                authority,
            )
            .await
            .unwrap();
            worker.shutdown().await.unwrap();
            let worker = Worker::reopen_whole(forward, configuration.clone(), authority)
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(
                    worker
                        .client()
                        .export_archive()
                        .unwrap()
                        .result()
                        .await
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(&after).unwrap()
            );
            assert_eq!(
                worker
                    .client()
                    .execute_signed(signed.clone(), UnixSeconds::new(1_950_000_000))
                    .unwrap()
                    .result()
                    .await
                    .unwrap(),
                expected
            );
            worker.shutdown().await.unwrap();
        });
        println!(
            "{name}: current writes, frozen rollback/further work, physical-cohort rejection and fresh roll-forward verified"
        );
    }
    fs::remove_dir_all(base).unwrap();
}
