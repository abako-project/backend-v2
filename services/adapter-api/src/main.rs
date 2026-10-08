//! Independent REST adapter; delivery state never owns business truth.
mod auth;
mod bramp;
mod catalog;
mod disputes;
mod http;
mod notifications;
mod operations;
mod profiles;
mod project_briefs;
mod state;

use state::{App, Config, Error};
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|_| Error::Internal)?;
    let config = tokio::task::spawn_blocking(Config::from_env).await??;
    let bind_addr = config.bind_addr.clone();
    let app = Arc::new(App::new(config).await?);
    auth::bootstrap(&app).await?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let (stop, stopping) = watch::channel(false);
    let mut tasks = JoinSet::new();
    tasks.spawn(operations::run(app.clone(), stopping.clone()));
    tasks.spawn(notifications::run(app.clone(), stopping.clone()));
    let mut server_stop = stopping;
    tasks.spawn(async move {
        axum::serve(listener, http::router(app))
            .with_graceful_shutdown(async move {
                if !*server_stop.borrow() {
                    let _ = server_stop.changed().await;
                }
            })
            .await
            .map_err(Error::from)
    });
    let outcome = tokio::select! {
        signal = shutdown() => signal,
        result = tasks.join_next() => match result {
            Some(Ok(result)) => result,
            Some(Err(error)) => Err(Error::from(error)),
            None => Err(Error::Internal),
        },
    };
    stop.send(true).map_err(|_| Error::Internal)?;
    // Idle SSE clients cannot hold shutdown forever.
    let drain = async {
        while let Some(result) = tasks.join_next().await {
            result??;
        }
        Ok::<_, Error>(())
    };
    if let Ok(result) = tokio::time::timeout(Duration::from_secs(10), drain).await {
        result?;
    } else {
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result
                && !error.is_cancelled()
            {
                return Err(error.into());
            }
        }
    }
    outcome
}

async fn shutdown() -> Result<(), Error> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(Error::from),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.map_err(Error::from)
}

#[cfg(test)]
mod tests;
