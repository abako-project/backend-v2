//! Internal-only custody service. This process signs but never submits calls.

mod crypto;
mod store;
mod web;

use generated_contracts::{WalletId, WalletLifecycle};
use std::{
    env, error::Error, future::IntoFuture, net::SocketAddr, path::Path, sync::Arc, time::Duration,
};
use tokio::{sync::watch, task::JoinSet};

use crate::{crypto::read_secret, store::Store};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "init-dev-secrets") && args.len() == 2 {
        return crypto::init_dev_secrets(Path::new(&args[1])).map_err(Into::into);
    }
    let lifecycle = if args.first().is_some_and(|arg| arg == "set-lifecycle") && args.len() == 3 {
        let wallet: WalletId = args[1].to_str().ok_or("invalid wallet ID")?.parse()?;
        let state = match args[2].to_str() {
            Some("Suspended") => WalletLifecycle::Suspended,
            Some("Retired") => WalletLifecycle::Retired,
            _ => return Err("lifecycle must be Suspended or Retired".into()),
        };
        Some((wallet, state))
    } else if args.is_empty() {
        None
    } else {
        return Err("usage: wallet [init-dev-secrets <directory> | set-lifecycle <wallet-id> <Suspended|Retired>]".into());
    };
    let master = read_secret(&env::var("CUSTODY_MASTER_KEY_FILE")?)?;
    let root_seed = read_secret(&env::var("CUSTODY_ROOT_SEED_FILE")?)?;
    let token = crypto::read_token(&env::var("INTERNAL_SERVICE_TOKEN_FILE")?)?;
    let database = env::var("CUSTODY_DATABASE_URL")?;
    let address: SocketAddr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8081".into())
        .parse()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            let store = Store::open(&database, Arc::new(master), root_seed).await?;
            if let Some((wallet, lifecycle)) = lifecycle {
                store.set_lifecycle(wallet, lifecycle).await?;
                store.close().await;
                return Ok(());
            }
            let state = web::AppState::new(store.clone(), token);
            let listener = tokio::net::TcpListener::bind(address).await?;
            let (stop, stopping) = watch::channel(false);
            let mut workers = JoinSet::new();
            for _ in 0..4 {
                workers.spawn(signing_worker(store.clone(), stopping.clone()));
            }
            let mut server_stop = stopping.clone();
            let server = axum::serve(listener, web::router(state))
                .with_graceful_shutdown(async move {
                    while !*server_stop.borrow() {
                        if server_stop.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .into_future();
            tokio::pin!(server);
            // A failed worker takes the service out of readiness instead of stranding jobs.
            tokio::select! {
                result = &mut server => result?,
                result = shutdown_signal() => {
                    result?;
                    stop.send(true)?;
                    (&mut server).await?;
                },
                result = workers.join_next() => {
                    result.ok_or("signing workers unavailable")???;
                }
            }
            workers.abort_all();
            while let Some(result) = workers.join_next().await {
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => return Err(error.into()),
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => return Err(error.into()),
                }
            }
            store.close().await;
            Ok::<(), Box<dyn Error>>(())
        })
}

async fn shutdown_signal() -> std::io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _signal = terminate.recv() => Ok(()),
    }
}

async fn signing_worker(
    store: Store,
    mut stopping: watch::Receiver<bool>,
) -> Result<(), store::Error> {
    let mut failures = 0u32;
    loop {
        if *stopping.borrow() {
            return Ok(());
        }
        // A lease survives cancellation or a crash and is reclaimed after 30 seconds.
        let delay = match store.process_one().await {
            Ok(_) => {
                failures = 0;
                Duration::from_millis(100)
            }
            Err(error) => {
                failures += 1;
                if failures >= 5 {
                    return Err(error);
                }
                Duration::from_secs(1 << (failures - 1))
            }
        };
        tokio::select! {
            () = tokio::time::sleep(delay) => {},
            _changed = stopping.changed() => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
