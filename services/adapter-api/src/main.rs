//! Independent REST adapter; delivery state never owns business truth.
mod auth;
mod bramp;
mod catalog;
mod catalog_claims;
mod catalog_metadata;
mod catalog_metadata_io;
mod creation_metadata;
mod creation_metadata_io;
mod disputes;
mod evidence;
mod http;
mod notifications;
mod operations;
mod platform;
mod profiles;
mod project_briefs;
mod project_content;
mod project_participants;
mod proposal_metadata;
mod proposal_metadata_io;
mod proposal_reviews;
mod reason_metadata;
mod reason_metadata_io;
mod state;
mod submission_comments;
mod submission_presentations;
mod task_history;
mod task_state;

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
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if owner_command(&app, &arguments).await? {
        return Ok(());
    }
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

async fn owner_command(app: &App, arguments: &[String]) -> Result<bool, Error> {
    match arguments {
        [] => Ok(false),
        [command] if command == "check-provider-cutover" => {
            creation_metadata_io::reconciled(app).await?;
            tracing::info!("Provider cutover queue is reconciled");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "import-catalog-metadata" => {
            let records = catalog_metadata_io::import(app, path.into()).await?;
            tracing::info!(records, "catalog metadata imported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "export-catalog-metadata" => {
            let records = catalog_metadata_io::export(app, path.into()).await?;
            tracing::info!(records, "catalog metadata exported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "import-reason-metadata" => {
            let records = reason_metadata_io::import(app, path.into()).await?;
            tracing::info!(records, "reason metadata imported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "export-reason-metadata" => {
            let records = reason_metadata_io::export(app, path.into()).await?;
            tracing::info!(records, "reason metadata exported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "import-proposal-metadata" => {
            let records = proposal_metadata_io::import(app, path.into()).await?;
            tracing::info!(records, "Proposal metadata imported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "export-proposal-metadata" => {
            let records = proposal_metadata_io::export(app, path.into()).await?;
            tracing::info!(records, "Proposal metadata exported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "import-creation-metadata" => {
            let records = creation_metadata_io::import(app, path.into()).await?;
            tracing::info!(records, "Creation metadata imported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command, path] if command == "export-creation-metadata" => {
            let records = creation_metadata_io::export(app, path.into()).await?;
            tracing::info!(records, "Creation metadata exported and verified");
            app.db.close().await;
            Ok(true)
        }
        [command] if command == "backfill-task-state" => {
            let (storages, tasks) = task_state::backfill(app).await?;
            tracing::info!(
                storages,
                tasks,
                "Provider task state copied and verified; ownership unchanged"
            );
            app.db.close().await;
            Ok(true)
        }
        [command, generation, path] if command == "export-prepared-tasks" => {
            task_state::export(app, state::parse(generation)?, path.into()).await?;
            tracing::info!("Prepared task state exported; no ownership or signature asserted");
            app.db.close().await;
            Ok(true)
        }
        [command] if command == "backfill-provider-content" => {
            let (workers, projects) = backfill_content(app).await?;
            tracing::info!(workers, projects, "Provider content copies verified");
            app.db.close().await;
            Ok(true)
        }
        _ => Err(Error::Config),
    }
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

/// Explicit operator command; reads the owner-provided snapshot over HTTP only.
async fn backfill_content(app: &App) -> Result<(usize, usize), Error> {
    let snapshot = app.snapshot().await?;
    let generation = snapshot.info.provider_instance_id;
    let counts = (snapshot.workers.len(), snapshot.projects.len());
    for worker in snapshot.workers {
        profiles::directory::compose(app, generation, worker).await?;
    }
    for project in snapshot.projects {
        project_content::compose(app, generation, project).await?;
    }
    Ok(counts)
}
