//! Private fresh-destination bridge; this API is never mounted in HTTP.
#![cfg(all(feature = "storage-memory", feature = "storage-sqlite"))]
use generated_contracts::AccountId32;
use mock_provider::Provider;
use std::path::PathBuf;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
struct Database(PathBuf);
impl Database {
    fn new() -> TestResult<Self> {
        // PID namespaces can reuse IDs across verification commands.
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| std::io::Error::other("test path entropy"))?;
        Ok(Self(std::env::temp_dir().join(format!(
            "provider-import-{}-{:032x}.sqlite",
            std::process::id(),
            u128::from_le_bytes(token)
        ))))
    }
    fn url(&self) -> String {
        format!("sqlite://{}", self.0.display())
    }
}
#[tokio::test]
async fn fresh_import_retains_generation_and_never_replaces_existing_state() -> TestResult {
    let root = AccountId32::from_bytes([7; 32]);
    let source = Provider::memory(root)?;
    let original = source.archive().await?;
    let mut retained = original.clone();
    retained.checkpoint.next_task_id = 999;
    // Imported catalogs must not be overwritten by startup's optional seed fixture.
    retained.checkpoint.skills.clear();
    retained.checkpoint.skill_metadata.clear();
    let destination = Database::new()?;
    let imported =
        Provider::sqlite_from_archive(&destination.url(), root, retained.clone()).await?;
    assert_eq!(
        serde_json::to_vec(&imported.archive().await?)?,
        serde_json::to_vec(&retained)?
    );
    let reopened = Provider::sqlite(&destination.url(), root).await?;
    assert_eq!(
        serde_json::to_vec(&reopened.archive().await?)?,
        serde_json::to_vec(&retained)?
    );
    let rejection = Provider::sqlite_from_archive(&destination.url(), root, original.clone())
        .await
        .err()
        .ok_or("replaced populated destination")?;
    assert_eq!(rejection.code(), "migration_destination_not_empty");
    assert_eq!(
        serde_json::to_vec(&imported.archive().await?)?,
        serde_json::to_vec(&retained)?
    );
    let restored = Provider::memory_from_archive(root, imported.archive().await?).await?;
    assert_eq!(
        serde_json::to_vec(&restored.archive().await?)?,
        serde_json::to_vec(&retained)?
    );
    let invalid = Database::new()?;
    let mut unsupported = original.clone();
    unsupported.version = 6;
    assert!(
        Provider::sqlite_from_archive(&invalid.url(), root, unsupported)
            .await
            .is_err()
    );
    // An aborted import leaves no generation row behind.
    let retry = Provider::sqlite_from_archive(&invalid.url(), root, original.clone()).await?;
    assert_eq!(
        serde_json::to_vec(&retry.archive().await?)?,
        serde_json::to_vec(&original)?
    );
    std::fs::remove_file(destination.0)?;
    std::fs::remove_file(invalid.0)?;
    Ok(())
}

#[tokio::test]
async fn unsupported_destination_format_is_never_overwritten() -> TestResult {
    let root = AccountId32::from_bytes([7; 32]);
    let source = Provider::memory(root)?;
    let destination = Database::new()?;
    let pool = sqlx::SqlitePool::connect(&format!("{}?mode=rwc", destination.url())).await?;
    sqlx::query("CREATE TABLE provider_state (singleton INTEGER PRIMARY KEY, format_version INTEGER NOT NULL, state BLOB NOT NULL)").execute(&pool).await?;
    sqlx::query("INSERT INTO provider_state VALUES (1,2,?)")
        .bind(b"retained future format".to_vec())
        .execute(&pool)
        .await?;
    let before: (i64, Vec<u8>) =
        sqlx::query_as("SELECT format_version,state FROM provider_state WHERE singleton=1")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        Provider::sqlite_from_archive(&destination.url(), root, source.archive().await?)
            .await
            .err()
            .ok_or("import replaced future format")?
            .code(),
        "migration_destination_not_empty"
    );
    assert_eq!(
        Provider::sqlite(&destination.url(), root)
            .await
            .err()
            .ok_or("startup replaced future format")?
            .code(),
        "state_configuration_mismatch"
    );
    let after: (i64, Vec<u8>) =
        sqlx::query_as("SELECT format_version,state FROM provider_state WHERE singleton=1")
            .fetch_one(&pool)
            .await?;
    assert_eq!(before, after);
    let extra_tables: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='provider_migration'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(extra_tables, 0);
    pool.close().await;
    std::fs::remove_file(destination.0)?;
    Ok(())
}

/// Run after the SDK trace writes `SANDBOX_ROLLBACK_OUTPUT`. This deliberately
/// consumes an observed sandbox export, never the pre-cutover native checkpoint.
#[tokio::test]
#[ignore = "Requires an observed current sandbox archive; see the migration verification gate"]
async fn rollback_current_wasm_state_accepts_new_native_work() -> TestResult {
    use generated_contracts::*;
    let bytes = tokio::fs::read(std::env::var("SANDBOX_ROLLBACK_ARCHIVE")?).await?;
    let archive: provider_execution::migration::CoreArchiveV5 = serde_json::from_slice(&bytes)?;
    let root = archive.checkpoint.info.root_account;
    let source_events = archive.checkpoint.events.len();
    let source_nonce = archive
        .checkpoint
        .nonces
        .get(&root)
        .copied()
        .unwrap_or_default();
    let memory = Provider::memory_from_archive(root, archive.clone()).await?;
    assert_eq!(serde_json::to_vec(&memory.archive().await?)?, bytes);
    let destination = Database::new()?;
    let restored = Provider::sqlite_from_archive(&destination.url(), root, archive.clone()).await?;
    assert_eq!(serde_json::to_vec(&restored.archive().await?)?, bytes);
    let call = UnsignedContractCallV1::new(
        archive.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([240; 16]),
        root,
        source_nonce,
        UnixSeconds::new(10_000),
        ProviderCommand::SetCompensationPolicy(CompensationPolicy {
            coordinator_percent: Percentage::new(20)?,
        }),
    )?;
    let key = subxt_signer::sr25519::Keypair::from_secret_key([1; 32])?;
    assert_eq!(root, AccountId32::from_bytes(key.public_key().0));
    let signed = SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
        call,
    };
    let receipt = restored.execute(signed, UnixSeconds::new(1_000)).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    let current = restored.archive().await?;
    assert_eq!(
        current.checkpoint.events.len(),
        source_events.checked_add(1).ok_or("event count")?
    );
    assert_eq!(
        current.checkpoint.nonces.get(&root),
        source_nonce.checked_add(1).as_ref()
    );
    assert_eq!(
        serde_json::to_vec(&current.checkpoint.projects)?,
        serde_json::to_vec(&archive.checkpoint.projects)?
    );
    assert_eq!(
        serde_json::to_vec(&current.checkpoint.bramp)?,
        serde_json::to_vec(&archive.checkpoint.bramp)?
    );
    assert_eq!(
        current.checkpoint.compensation_policy.coordinator_percent,
        Percentage::new(20)?
    );
    let reopened = Provider::sqlite(&destination.url(), root).await?;
    assert_eq!(
        serde_json::to_vec(&reopened.archive().await?)?,
        serde_json::to_vec(&current)?
    );
    std::fs::remove_file(destination.0)?;
    Ok(())
}
