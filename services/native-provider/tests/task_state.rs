//! The task migration export stays private and does not execute a command.
#![cfg(feature = "storage-memory")]
use native_provider::Provider;
use protocol::{AccountId32, provider_core::TaskState};

#[tokio::test]
async fn task_state_http_requires_service_identity_and_leaves_shell_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let root = AccountId32::from_bytes([8; 32]);
    let provider = Provider::memory(root)?;
    let info = provider.info().await?;
    let token = "task-state-test-service-credential-32-bytes";
    let router = native_provider::router(provider.clone(), token)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/internal/task-state", listener.local_addr()?);
    let (shutdown, stopping) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                // Either an explicit stop or a dropped sender ends this test server.
                stopping.await.unwrap_or(());
            })
            .await
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client.get(&url).send().await?.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let allocation_url = url.replace(
        "/internal/task-state",
        &format!(
            "/internal/task-allocations/{}",
            protocol::OperationId::from_bytes([7; 16]),
        ),
    );
    assert_eq!(
        client.get(&allocation_url).send().await?.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(&allocation_url)
            .bearer_auth(token)
            .send()
            .await?
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let response = client.get(&url).bearer_auth(token).send().await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let state = response.json::<TaskState>().await?;
    state.validate()?;
    assert_eq!(state.generation, info.provider_instance_id);
    assert_eq!(state.next_task_id, 1);
    assert!(!state.has_task_history);
    assert!(state.storages.is_empty());
    assert_eq!(provider.info().await?, info);
    assert_eq!(provider.nonce(root).await?.nonce, 0);
    assert!(provider.events(0, 1).await?.events.is_empty());
    shutdown.send(()).map_err(|()| "server already stopped")?;
    server.await??;
    Ok(())
}
