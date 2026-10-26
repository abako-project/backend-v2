//! Offline migration uses protected new files and rolls back from current state.
use generated_contracts::*;
use ink_sandbox::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::ReferenceState;
use provider_execution::{
    ExecutionInputs, VerifiedCall,
    migration::{CoreArchiveV4, CoreArchiveV5},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn invoke(database: &Path, root: &Path, authority: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_whole-provider"))
        .args(args)
        .env("WHOLE_DATABASE_PATH", database)
        .env("MOCK_ROOT_ACCOUNT_FILE", root)
        .env("PROVIDER_AUTHORITY", authority)
        .output()
        .unwrap()
}
#[test]
fn offline_cli_preserves_native_source_and_rolls_back_current_sandbox_writes() {
    let directory = Directory(std::env::temp_dir().join(format!("ink-cli-{}", std::process::id())));
    fs::create_dir(&directory.0).unwrap();
    let reference: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    let initial: CoreArchiveV4 = serde_json::from_str(
        reference["archives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["beforeStep"] == 70)
            .unwrap()["archive"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let initial = CoreArchiveV5::from_v4(initial).unwrap().0;
    let root = directory.0.join("root.hex");
    fs::write(&root, initial.checkpoint.info.root_account.to_string()).unwrap();
    let source = directory.0.join("native.sqlite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc",source.display())).await.unwrap();
        sqlx::query("CREATE TABLE provider_state(singleton INTEGER PRIMARY KEY,format_version INTEGER NOT NULL,state BLOB NOT NULL)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_state VALUES(1,1,?)").bind(serde_json::to_vec(&initial.checkpoint).unwrap()).execute(&pool).await.unwrap();
        pool.close().await;
    });
    let native_bytes = fs::read(&source).unwrap();
    let exported = directory.0.join("native.json");
    let result = invoke(
        &source,
        &root,
        "rust",
        &["export-native", exported.to_str().unwrap()],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        native_bytes,
        "read-only export must not reseed or alter schema"
    );
    assert_eq!(
        serde_json::to_vec(
            &serde_json::from_slice::<CoreArchiveV5>(&fs::read(&exported).unwrap()).unwrap()
        )
        .unwrap(),
        serde_json::to_vec(&initial).unwrap()
    );
    let sandbox = directory.0.join("sandbox.sqlite");
    let result = invoke(
        &sandbox,
        &root,
        "sandbox",
        &["import", exported.to_str().unwrap()],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&exported, &sandbox] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let configuration = WholeConfiguration {
        root: initial.checkpoint.info.root_account,
        generation: initial.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    };
    let trace: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
    ))
    .unwrap();
    let mut expected = ReferenceState::from_archive(configuration.root, initial).unwrap();
    expected = runtime.block_on(async {
        let worker =
            Worker::reopen_whole(sandbox.clone(), configuration.clone(), Authority::Sandbox)
                .await
                .unwrap();
        for step in &trace["steps"].as_array().unwrap()[70..72] {
            let call: SignedContractCallV1 =
                serde_json::from_value(step["signed"].clone()).unwrap();
            let inputs: ExecutionInputs = serde_json::from_value(step["inputs"].clone()).unwrap();
            let (next, receipt) = expected
                .execute(VerifiedCall::verify(call.clone()).unwrap(), inputs.clone())
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
        }
        worker.shutdown().await.unwrap();
        expected
    });
    let current = directory.0.join("current.json");
    let result = invoke(
        &sandbox,
        &root,
        "sandbox",
        &["export", current.to_str().unwrap()],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&current).unwrap();
    assert_eq!(
        bytes,
        serde_json::to_vec(&expected.archive().unwrap()).unwrap()
    );
    assert!(
        !invoke(
            &sandbox,
            &root,
            "sandbox",
            &["export", current.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(&current).unwrap(), bytes);
    let rollback = directory.0.join("rollback.sqlite");
    assert!(
        invoke(
            &rollback,
            &root,
            "rust",
            &["import", current.to_str().unwrap()]
        )
        .status
        .success()
    );
    let before = fs::read(&rollback).unwrap();
    assert!(
        !invoke(
            &rollback,
            &root,
            "rust",
            &["import", current.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(&rollback).unwrap(), before);
    runtime.block_on(async {
        let worker = Worker::reopen_whole(rollback, configuration, Authority::Rust)
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
            bytes
        );
        let step = &trace["steps"][71];
        assert_eq!(
            serde_json::to_value(
                worker
                    .client()
                    .execute_directory_with_inputs(
                        serde_json::from_value(step["signed"].clone()).unwrap(),
                        serde_json::from_value(step["inputs"].clone()).unwrap()
                    )
                    .unwrap()
                    .result()
                    .await
                    .unwrap()
            )
            .unwrap(),
            step["outcome"]["receipt"]
        );
        worker.shutdown().await.unwrap();
    });
    assert!(
        !invoke(&source, &root, "rust", &["diagnostics"])
            .status
            .success()
    );
    assert_eq!(fs::read(source).unwrap(), native_bytes);
}

#[test]
fn explicit_v3_upgrade_and_downgrade_cli_preserve_owner_files_and_reject_wrong_metadata() {
    let directory = Directory(std::env::temp_dir().join(format!(
            "ink-core-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir(&directory.0).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/catalog-predecessor.json"
    ))
    .unwrap();
    let source = fixture["archives"][1]["archive"]
        .as_str()
        .unwrap()
        .as_bytes();
    let before: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_slice(source).unwrap();
    let root = directory.0.join("root.hex");
    fs::write(&root, before.checkpoint.info.root_account.to_string()).unwrap();
    let input = directory.0.join("v3.json");
    fs::write(&input, source).unwrap();
    let core = directory.0.join("core.json");
    let metadata = directory.0.join("metadata.ndjson");
    let reasons = directory.0.join("reasons.ndjson");
    let database = directory.0.join("unused.sqlite");
    let result = invoke(
        &database,
        &root,
        "rust",
        &[
            "upgrade",
            input.to_str().unwrap(),
            core.to_str().unwrap(),
            metadata.to_str().unwrap(),
            reasons.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!database.exists());
    assert_eq!(fs::read(&input).unwrap(), source);
    let current: CoreArchiveV4 = serde_json::from_slice(&fs::read(&core).unwrap()).unwrap();
    assert_eq!(current.version, 4);
    assert_eq!(current.checkpoint.info, before.checkpoint.info);
    let output = directory.0.join("back.json");
    let result = invoke(
        &database,
        &root,
        "rust",
        &[
            "downgrade",
            core.to_str().unwrap(),
            metadata.to_str().unwrap(),
            reasons.to_str().unwrap(),
            output.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&output).unwrap()).unwrap(),
        serde_json::from_slice::<Value>(source).unwrap()
    );
    assert!(
        !invoke(
            &database,
            &root,
            "rust",
            &[
                "upgrade",
                input.to_str().unwrap(),
                core.to_str().unwrap(),
                metadata.to_str().unwrap(),
                reasons.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let original = fs::read_to_string(&metadata).unwrap();
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();
    let mut header: Value = serde_json::from_str(&lines[0]).unwrap();
    header["generation"] = serde_json::to_value(ProviderInstanceId::from_bytes([77; 16])).unwrap();
    lines[0] = serde_json::to_string(&header).unwrap();
    let wrong = directory.0.join("wrong.ndjson");
    fs::write(&wrong, lines.join("\n") + "\n").unwrap();
    let rejected = directory.0.join("rejected.json");
    assert!(
        !invoke(
            &database,
            &root,
            "rust",
            &[
                "downgrade",
                core.to_str().unwrap(),
                wrong.to_str().unwrap(),
                reasons.to_str().unwrap(),
                rejected.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    assert!(!rejected.exists());
    let duplicate = directory.0.join("duplicate.ndjson");
    let extra = original.lines().nth(1).unwrap();
    fs::write(&duplicate, format!("{original}{extra}\n")).unwrap();
    assert!(
        !invoke(
            &database,
            &root,
            "rust",
            &[
                "downgrade",
                core.to_str().unwrap(),
                duplicate.to_str().unwrap(),
                rejected.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    assert!(!rejected.exists());
    assert_eq!(fs::read(&input).unwrap(), source);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&core, &metadata, &output] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn current_metadata_recovery_reads_v5_with_compact_tasks_and_selected_legacy_writes() {
    use generated_contracts::provider_core::{CatalogContentV1, CatalogMetadataRecordV1};
    use sp_core::{Pair, sr25519};
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    let original = fixture["archives"].as_array().unwrap().last().unwrap()["archive"]
        .as_str()
        .unwrap();
    let old: CoreArchiveV4 = serde_json::from_str(original).unwrap();
    let (core, _) =
        mock_provider::migration::compact_tasks(old.checkpoint.info.root_account, old).unwrap();
    assert!(
        core.checkpoint
            .projects
            .values()
            .flat_map(|p| &p.proposals)
            .flat_map(|p| &p.milestones)
            .any(|m| !m.task_storage.tasks.is_empty())
    );
    let key = sr25519::Pair::from_seed(&[1; 32]);
    let origin = AccountId32::from_bytes(key.public().0);
    assert_eq!(origin, core.checkpoint.info.root_account);
    let call = UnsignedContractCallV1::new(
        core.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([249; 16]),
        origin,
        core.checkpoint.nonces[&origin],
        UnixSeconds::new(u64::MAX),
        ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: 1,
            name: "Recovered after task cutover\0".into(),
        }),
    )
    .unwrap();
    let signed = SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0),
        call,
    };
    let (current, receipt) = ReferenceState::from_archive(origin, core)
        .unwrap()
        .execute(
            VerifiedCall::verify(signed).unwrap(),
            ExecutionInputs {
                version: 1,
                now: UnixSeconds::new(1_900_000_000),
                seed: [249; 32],
            },
        )
        .unwrap();
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    let current = current.archive().unwrap();
    let directory = Directory(
        std::env::temp_dir().join(format!("v5-metadata-recovery-{}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    let root = directory.0.join("root.hex");
    fs::write(&root, origin.to_string()).unwrap();
    let database = directory.0.join("unused.sqlite");
    for (version, bytes) in [
        (4, original.as_bytes().to_vec()),
        (5, serde_json::to_vec(&current).unwrap()),
    ] {
        let source = directory.0.join(format!("v{version}.json"));
        fs::write(&source, &bytes).unwrap();
        for kind in ["creation", "proposal", "catalog", "reason"] {
            let output = directory.0.join(format!("{version}-{kind}.ndjson"));
            let result = invoke(
                &database,
                &root,
                "rust",
                &[
                    &format!("recover-{kind}-metadata"),
                    source.to_str().unwrap(),
                    output.to_str().unwrap(),
                ],
            );
            assert!(
                result.status.success(),
                "v{version}/{kind}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let records = fs::read_to_string(&output).unwrap();
            let lines: Vec<_> = records.lines().collect();
            assert!(lines.len() > 1, "selected {kind} records must be recovered");
            let header: Value = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(
                header["generation"],
                serde_json::to_value(current.checkpoint.info.provider_instance_id).unwrap()
            );
            if version == 5 && kind == "catalog" {
                assert!(lines[1..].iter().any(|line| {
                    let record: CatalogMetadataRecordV1 = serde_json::from_str(line).unwrap();
                    matches!(record.content, CatalogContentV1::Entry { kind: CatalogKind::Skill, id: 1, name, .. }
                        if name == "Recovered after task cutover\0")
                }));
            }
            assert_eq!(fs::read(&source).unwrap(), bytes);
            assert!(!database.exists());
        }
    }
}
