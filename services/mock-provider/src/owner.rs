//! Explicit owner file operations. No owner routes are mounted in HTTP.
use generated_contracts::{AccountId32, provider_core::TaskStateV1};
use mock_provider::migration::restore_task_content;
use provider_execution::migration::CoreArchiveV5;
use std::{
    error::Error,
    fs,
    io::{Read, Write},
    path::PathBuf,
};
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn read<T: serde::de::DeserializeOwned>(path: PathBuf, maximum: u64) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > maximum {
        return Err("owner input exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid owner input".into())
}
fn write(path: PathBuf, archive: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(archive).map_err(|_| "cannot encode archive")?;
    if bytes.len() > 67_108_864 {
        return Err("archive exceeds limit".into());
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(crate) async fn restore(
    archive: PathBuf,
    tasks: PathBuf,
    output: PathBuf,
    root: AccountId32,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let archive: CoreArchiveV5 = read(archive, 67_108_864)?;
        let tasks: TaskStateV1 = read(tasks, 33_554_432)?;
        write(output, &restore_task_content(root, archive, tasks)?)
    })
    .await?
}
/// Convert only after the adapter's prepared export agrees with the full source.
pub(crate) async fn compact(
    archive: PathBuf,
    tasks: PathBuf,
    output: PathBuf,
    root: AccountId32,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let archive: provider_execution::migration::CoreArchiveV4 = read(archive, 67_108_864)?;
        let prepared: TaskStateV1 = read(tasks, 33_554_432)?;
        let (compact, actual) = mock_provider::migration::compact_tasks(root, archive)?;
        if actual != prepared {
            return Err("prepared task state does not match predecessor archive".into());
        }
        write(output, &compact)
    })
    .await?
}
#[cfg(feature = "storage-sqlite")]
pub(crate) async fn export(url: String, output: PathBuf, root: AccountId32) -> Result<()> {
    use mock_provider::ReferenceState;
    use provider_execution::migration::CoreCheckpointV5;
    use sqlx::{
        Row,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::str::FromStr;
    // Opening the normal serving constructor could seed catalogs. A read-only
    // connection exports only this owner's existing store without startup writes.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str(&url)?
                .read_only(true)
                .create_if_missing(false),
        )
        .await?;
    let result=sqlx::query("SELECT state FROM provider_state WHERE singleton=1 AND format_version=1 AND length(state)<=67108864").fetch_one(&pool).await;
    pool.close().await;
    let bytes: Vec<u8> = result?.try_get("state")?;
    tokio::task::spawn_blocking(move || {
        let checkpoint: CoreCheckpointV5 =
            serde_json::from_slice(&bytes).map_err(|_| "invalid stored archive")?;
        let archive = CoreArchiveV5 {
            version: 5,
            checkpoint,
        };
        let exact = serde_json::to_vec(&archive)?;
        let validated = ReferenceState::from_archive(root, archive)?.archive()?;
        if serde_json::to_vec(&validated)? != exact {
            return Err("stored archive requires normalization".into());
        }
        write(output, &validated)
    })
    .await?
}
