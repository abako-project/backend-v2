use super::*;
use protocol::PlatformProfile;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

#[tokio::test]
async fn platform_is_public_and_works_without_database_or_provider() -> TestResult {
    let dependency = Server::start(Router::new()).await?;
    let config = test_config("postgres://unused@127.0.0.1:1/unused".into(), &dependency)?;
    let db = sqlx::postgres::PgPoolOptions::new().connect_lazy(&config.database_url)?;
    let app = Arc::new(App {
        config,
        db,
        client: reqwest::Client::new(),
        password_slots: Arc::new(Semaphore::new(1)),
        request_slots: Semaphore::new(4),
        stream_slots: Arc::new(Semaphore::new(1)),
    });
    dependency.finish().await?;
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/api/platform", server.url);
    let response = client.get(&url).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert_eq!(
        response.json::<PlatformProfile>().await?,
        app.config.platform
    );
    let response = client
        .get(&url)
        .header(header::ORIGIN, "http://localhost:3000")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "http://localhost:3000"
    );
    assert_eq!(client.head(&url).send().await?.status(), StatusCode::OK);
    assert_eq!(
        client
            .get(&url)
            .header(header::ORIGIN, "https://untrusted.test")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client.post(&url).json(&json!({})).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    server.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn briefs_accept_configured_types_preserve_legacy_and_read_retired_types() -> TestResult {
    let fake = fake()?;
    let dependency = Server::start(internal(fake.clone())).await?;
    let app = app(&dependency).await?;
    let (owner, cookie) = register(&app, "platform_brief_client").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = owner.view.account_id;
    let id = project.project_id;
    fake.lock()
        .await
        .snapshot
        .projects
        .push(stage_project(&app, info().provider_instance_id, project).await?);
    let mut config = test_config(app.config.database_url.clone(), &dependency)?;
    config.platform.project_types = vec!["Building Design".into()];
    let configured = Arc::new(App::new(config).await?);
    let server = Server::start(http::router(configured.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/api/projects/{id}/brief", server.url);
    let mut request = doc["components"]["schemas"]["PutProjectBriefRequest"]["examples"][0].clone();
    request["expectedRevision"] = json!(0);
    for (index, value) in ["SmartContract", "Full Stack", "Building Design"]
        .into_iter()
        .enumerate()
    {
        request["brief"]["projectType"] = json!(value);
        let response = client
            .put(&url)
            .header(header::COOKIE, &cookie)
            .header("X-CSRF-Token", &owner.view.csrf_token)
            .json(&request)
            .send()
            .await?;
        if index == 1 {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            continue;
        }
        assert_eq!(
            response.status(),
            if index == 0 {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            }
        );
        let saved: Value = response.json().await?;
        assert_eq!(saved["brief"]["projectType"], value);
        request["expectedRevision"] = saved["revision"].clone();
    }
    server.finish().await?;
    configured.db.close().await;
    let restarted =
        Arc::new(App::new(test_config(app.config.database_url.clone(), &dependency)?).await?);
    let server = Server::start(http::router(restarted.clone())).await?;
    let saved: Value = client
        .get(format!("{}/api/projects/{id}/brief", server.url))
        .header(header::COOKIE, &cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(saved["brief"]["projectType"], "Building Design");
    assert_eq!(fake.lock().await.submissions, 0);
    server.finish().await?;
    restarted.db.close().await;
    app.db.close().await;
    dependency.finish().await?;
    Ok(())
}
