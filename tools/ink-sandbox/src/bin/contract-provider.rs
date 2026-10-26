//! Local contract provider, initialized through constructors and persisted as one current state.
use generated_contracts::AccountId32;
use ink_sandbox::{
    EXECUTION_GAS,
    worker::{Configuration, Worker},
};
use mock_provider::router_with_provider;
use std::{env, error::Error, fs, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
fn secret(name: &str) -> Result<String> {
    Ok(fs::read_to_string(env::var(name)?)?.trim().to_owned())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if !args.is_empty() && args != ["serve"] {
        return Err("usage: contract-provider [serve]".into());
    }
    let root: AccountId32 = secret("MOCK_ROOT_ACCOUNT_FILE")?.parse()?;
    let token = secret("INTERNAL_SERVICE_TOKEN_FILE")?;
    let path = PathBuf::from(
        env::var("CONTRACT_DATABASE_PATH").unwrap_or_else(|_| "mock-sandbox.sqlite".into()),
    );
    let mock_funding = match env::var("ENABLE_MOCK_FUNDING").as_deref() {
        Ok("true") => true,
        Ok("false") | Err(_) => false,
        _ => return Err("invalid ENABLE_MOCK_FUNDING".into()),
    };
    let worker = Worker::open(
        path,
        Configuration {
            root,
            mock_funding,
            gas_limit: EXECUTION_GAS,
        },
    )
    .await?;
    let result: Result<()> = async {
        let router = router_with_provider(worker.client(), &token)?;
        let listener = tokio::net::TcpListener::bind(
            env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8082".into()),
        )
        .await?;
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown())
            .await?;
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
