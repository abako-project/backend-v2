//! Explicit local candidate and offline migration tools; no production deployment.
#[path = "whole_provider/metadata.rs"]
mod metadata;
use generated_contracts::AccountId32;
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, MAX_SHELL, WholeConfiguration, Worker},
};
use mock_provider::{Provider, ReferenceState, router_with_provider};
use provider_execution::migration::{CoreArchiveV5, CoreCheckpointV5};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
fn secret(name: &str) -> Result<String> {
    Ok(fs::read_to_string(env::var(name)?)?.trim().to_owned())
}
fn configuration(archive: &CoreArchiveV5, root: AccountId32) -> Result<WholeConfiguration> {
    if archive.checkpoint.info.root_account != root {
        return Err("configured root differs from retained root".into());
    }
    Ok(WholeConfiguration {
        root,
        generation: archive.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    })
}
fn load_archive(path: &Path) -> Result<CoreArchiveV5> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SHELL as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SHELL {
        return Err("archive exceeds limit".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
fn write_archive(path: &Path, archive: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(archive)?;
    if bytes.len() > MAX_SHELL {
        return Err("archive exceeds limit".into());
    }
    let mut options = OpenOptions::new();
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
fn reserve_database(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.sync_all()?;
    Ok(())
}
async fn stored_archive(path: &Path, native: bool, root: AccountId32) -> Result<CoreArchiveV5> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await?;
    let query = if native {
        "SELECT state FROM provider_state WHERE singleton=1 AND format_version=1 AND length(state)<=67108864"
    } else {
        "SELECT archive FROM candidate_reference WHERE singleton=1 AND length(archive)<=67108864"
    };
    let bytes: Vec<u8> = sqlx::query_scalar(query).fetch_one(&pool).await?;
    pool.close().await;
    tokio::task::spawn_blocking(move || -> Result<CoreArchiveV5> {
        let archive = if native {
            CoreArchiveV5 {
                version: 5,
                checkpoint: serde_json::from_slice::<CoreCheckpointV5>(&bytes)?,
            }
        } else {
            serde_json::from_slice(&bytes)?
        };
        Ok(ReferenceState::from_archive(root, archive)?.archive()?)
    })
    .await?
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let command = args.first().map_or("serve", String::as_str);
    let path = PathBuf::from(
        env::var("WHOLE_DATABASE_PATH").unwrap_or_else(|_| "mock-sandbox.sqlite".into()),
    );
    let root: AccountId32 = secret("MOCK_ROOT_ACCOUNT_FILE")?.parse()?;
    if matches!(
        command,
        "upgrade"
            | "downgrade"
            | "recover-creation-metadata"
            | "recover-proposal-metadata"
            | "recover-catalog-metadata"
            | "recover-reason-metadata"
    ) {
        return tokio::task::spawn_blocking(move || metadata::run(&args, root)).await?;
    }
    let authority = match env::var("PROVIDER_AUTHORITY")
        .unwrap_or_else(|_| "rust".into())
        .as_str()
    {
        "rust" => Authority::Rust,
        "shadow" => Authority::Shadow,
        "sandbox" => Authority::Sandbox,
        _ => return Err("invalid PROVIDER_AUTHORITY".into()),
    };
    if !matches!(
        command,
        "serve" | "import" | "export" | "export-native" | "diagnostics"
    ) || args.len()
        != match command {
            "import" | "export" | "export-native" => 2,
            "serve" if args.is_empty() => 0,
            _ => 1,
        }
    {
        return Err("usage: whole-provider [serve | import ARCHIVE | export ARCHIVE | export-native ARCHIVE | diagnostics]".into());
    }
    if command == "export-native" {
        let archive = stored_archive(&path, true, root).await?;
        let output = PathBuf::from(&args[1]);
        tokio::task::spawn_blocking(move || write_archive(&output, &archive)).await??;
        return Ok(());
    }
    if command == "import" && path.exists() {
        return Err("import destination must be a new file".into());
    }
    let worker = if command == "import" {
        let input = PathBuf::from(&args[1]);
        let archive = tokio::task::spawn_blocking(move || load_archive(&input)).await??;
        let configuration = configuration(&archive, root)?;
        reserve_database(&path)?;
        Worker::import_whole(path, archive, configuration, authority).await?
    } else if path.exists() {
        let archive = stored_archive(&path, false, root).await?;
        Worker::reopen_whole(path, configuration(&archive, root)?, authority).await?
    } else if command == "serve" {
        let provider = tokio::task::spawn_blocking(move || Provider::memory(root)).await??;
        let archive = provider.archive().await?;
        let configuration = configuration(&archive, root)?;
        reserve_database(&path)?;
        Worker::import_whole(path, archive, configuration, authority).await?
    } else {
        return Err("existing whole-provider database required".into());
    };
    let result: Result<()> = async {
        match command {
            "serve" => {
                let token = secret("INTERNAL_SERVICE_TOKEN_FILE")?;
                let router = router_with_provider(worker.client(), &token)?;
                let listener = tokio::net::TcpListener::bind(
                    env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8082".into()),
                )
                .await?;
                axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown())
                    .await?;
            }
            "export" => {
                let archive = worker.client().export_archive()?.result().await?;
                let output = PathBuf::from(&args[1]);
                tokio::task::spawn_blocking(move || write_archive(&output, &archive)).await??;
            }
            "diagnostics" => println!(
                "{}",
                serde_json::to_string(&worker.client().whole_diagnostics()?.result().await?)?
            ),
            "import" => {}
            _ => return Err("unsupported command".into()),
        }
        Ok(())
    }
    .await;
    let stopped = worker.shutdown().await;
    result?;
    stopped?;
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { result = tokio::signal::ctrl_c() => { if result.is_err() { eprintln!("cannot receive interrupt signal; shutting down"); } }, _ = terminate.recv() => {} }
            return;
        }
    }
    if tokio::signal::ctrl_c().await.is_err() {
        eprintln!("cannot receive interrupt signal; shutting down");
    }
}
