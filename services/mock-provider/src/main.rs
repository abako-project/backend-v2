//! Private local smart-contract simulator; never deploy this runtime to production.

use std::{env, error::Error, fs};

use generated_contracts::AccountId32;
use mock_provider::{Provider, router};

fn secret(name: &str) -> Result<String, Box<dyn Error>> {
    let path = env::var(name).map_err(|_| format!("missing {name}"))?;
    let value = fs::read_to_string(path).map_err(|_| format!("cannot read {name}"))?;
    Ok(value.trim().to_owned())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|_| std::io::Error::other("tracing initialization failed"))?;
    let root: AccountId32 = secret("MOCK_ROOT_ACCOUNT_FILE")?.parse()?;
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() {
        #[cfg(feature = "storage-sqlite")]
        if let [command, archive, destination] = args.as_slice()
            && command == "import"
        {
            import_native(archive.into(), destination.into(), root).await?;
            return Ok(());
        }
        return Err("usage: mock-provider [import CORE_ARCHIVE NEW_DATABASE_FILE]".into());
    }
    let service_token = secret("INTERNAL_SERVICE_TOKEN_FILE")?;
    let mode = env::var("MOCK_STORAGE").unwrap_or_else(|_| {
        if cfg!(feature = "storage-sqlite") {
            "sqlite"
        } else {
            "memory"
        }
        .into()
    });
    let provider = match mode.as_str() {
        #[cfg(feature = "storage-memory")]
        "memory" => Provider::memory(root)?,
        #[cfg(feature = "storage-sqlite")]
        "sqlite" => {
            Provider::sqlite(
                &env::var("MOCK_DATABASE_URL")
                    .unwrap_or_else(|_| "sqlite://mock.sqlite?mode=rwc".into()),
                root,
            )
            .await?
        }
        _ => return Err("requested mock storage feature is not compiled".into()),
    };
    let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8082".into());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, router(provider, &service_token)?)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
async fn import_native(
    archive: std::path::PathBuf,
    destination: std::path::PathBuf,
    root: AccountId32,
) -> Result<(), Box<dyn Error>> {
    let url = format!("sqlite://{}", destination.display());
    let archive = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        fs::File::open(archive)?
            .take(67_108_865)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 67_108_864 {
            return Err(std::io::Error::other("archive exceeds limit"));
        }
        let archive: provider_execution::migration::CoreArchiveV2 = serde_json::from_slice(&bytes)
            .map_err(|_| std::io::Error::other("invalid core archive"))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(destination)?.sync_all()?;
        Ok::<_, std::io::Error>(archive)
    })
    .await??;
    Provider::sqlite_from_archive(&url, root, archive).await?;
    Ok(())
}

#[allow(clippy::print_stderr)] // Sanitized process diagnostics, never request or secret content.
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                result = tokio::signal::ctrl_c() => { if result.is_err() { eprintln!("Cannot receive interrupt signal; shutting down"); } },
                _ = terminate.recv() => {}
            }
        } else {
            eprintln!("Cannot receive termination signal; shutting down");
        }
    }
    #[cfg(not(unix))]
    if tokio::signal::ctrl_c().await.is_err() {
        eprintln!("Cannot receive interrupt signal; shutting down");
    }
}
