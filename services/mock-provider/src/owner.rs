//! Explicit owner file operations. No owner routes are mounted in HTTP.
use generated_contracts::{AccountId32, provider_core::TaskStateV1};
use mock_provider::migration::restore_tasks;
use provider_execution::migration::CoreArchiveV4;
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
fn write(path: PathBuf, archive: &CoreArchiveV4) -> Result<()> {
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
        let archive: CoreArchiveV4 = read(archive, 67_108_864)?;
        let tasks: TaskStateV1 = read(tasks, 33_554_432)?;
        write(output, &restore_tasks(root, &archive, tasks)?)
    })
    .await?
}
#[cfg(feature = "storage-sqlite")]
pub(crate) async fn export(url: String, output: PathBuf, root: AccountId32) -> Result<()> {
    use mock_provider::ReferenceState;
    use provider_execution::migration::CoreCheckpointV4;
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
        let checkpoint: CoreCheckpointV4 =
            serde_json::from_slice(&bytes).map_err(|_| "invalid stored archive")?;
        let archive = CoreArchiveV4 {
            version: 4,
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
