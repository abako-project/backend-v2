//! Private fresh-destination bridge; this API is never mounted in HTTP.
#![cfg(all(feature = "storage-memory", feature = "storage-sqlite"))]
use generated_contracts::AccountId32;
use mock_provider::Provider;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "provider-import-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
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
    let destination = Database::new();
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
    let restored = Provider::memory_from_archive(root, imported.archive().await?)?;
    assert_eq!(
        serde_json::to_vec(&restored.archive().await?)?,
        serde_json::to_vec(&retained)?
    );
    let invalid = Database::new();
    let mut unsupported = original.clone();
    unsupported.version = 2;
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
