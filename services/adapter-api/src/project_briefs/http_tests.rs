use super::*;
use crate::project_briefs::models::{ProjectBriefView, PutProjectBriefRequest};
use serde_json::{Value, json};

fn brief_request(
    revision: i64,
    summary: &str,
) -> Result<PutProjectBriefRequest, serde_json::Error> {
    serde_json::from_value(json!({
        "expectedRevision":revision,
        "brief":{
            "summary":summary,"projectType":"MVP","link":"https://example.test/brief",
            "objectives":["Ship the first feature","Document it"],"constraints":["Accessible"],
            "indicativeBudget":{"currency":"USD","range":"Below10000"},
            "delivery":{"preference":"SpecificDate","date":"2028-02-29"}
        }
    }))
}

fn put_brief(
    client: &reqwest::Client,
    url: &str,
    session: &Session,
    cookie: &str,
) -> reqwest::RequestBuilder {
    client
        .put(url)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:3000")
        .header("X-CSRF-Token", &session.view.csrf_token)
}

#[tokio::test]
async fn project_briefs_are_private_persistent_and_do_not_change_provider_state() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, owner_cookie) = register(&app, "brief_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "brief_coordinator").await?;
    let (_, outsider_cookie) = register(&app, "brief_outsider").await?;
    let (worker, worker_cookie) = register(&app, "brief_worker").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = owner.view.account_id;
    project.coordinator = coordinator.view.account_id;
    // An assigned worker follows the same project visibility rule as GET /projects/{id}.
    let mut definition = doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone();
    definition["assignees"] = json!([worker.view.account_id]);
    let task: TaskView = serde_json::from_value(json!({
        "taskId":1,"reporter":worker.view.account_id,"createdAt":1,"updatedAt":1,"task":definition
    }))?;
    project.proposals[0].milestones[0]
        .task_storage
        .tasks
        .push(task);
    let project_id = project.project_id;
    fake.lock().await.snapshot.projects.push(project.clone());
    let server = Server::start(http::router(app.clone())).await?;
    let url = format!("{}/api/projects/{project_id}/brief", server.url);
    let client = reqwest::Client::new();
    assert_eq!(
        client.get(&url).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &outsider_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        Value::Null
    );
    let request = brief_request(0, "Original brief")?;
    assert_eq!(
        client
            .put(&url)
            .header(header::COOKIE, &owner_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .put(&url)
            .header(header::COOKIE, &owner_cookie)
            .header("X-CSRF-Token", &owner.view.csrf_token)
            .header(header::ORIGIN, "https://untrusted.test")
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put_brief(&client, &url, &coordinator, &coordinator_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put_brief(&client, &url, &worker, &worker_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = put_brief(&client, &url, &owner, &owner_cookie)
        .json(&request)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let saved = response.json::<ProjectBriefView>().await?;
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.brief, request.brief);
    for cookie in [&owner_cookie, &coordinator_cookie, &worker_cookie] {
        assert_eq!(
            client
                .get(&url)
                .header(header::COOKIE, cookie)
                .send()
                .await?
                .json::<ProjectBriefView>()
                .await?,
            saved
        );
    }
    let retry = put_brief(&client, &url, &owner, &owner_cookie)
        .json(&request)
        .send()
        .await?;
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(retry.json::<ProjectBriefView>().await?, saved);
    let stale = put_brief(&client, &url, &owner, &owner_cookie)
        .json(&brief_request(0, "Different brief")?)
        .send()
        .await?;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(
        stale.json::<ApiError>().await?.code,
        "brief_revision_conflict"
    );
    let mut invalid = request.clone();
    invalid.expected_revision = 1;
    invalid.brief.summary = "a".repeat(281);
    assert_eq!(
        put_brief(&client, &url, &owner, &owner_cookie)
            .json(&invalid)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    invalid.brief.summary = "null\0byte".into();
    assert_eq!(
        put_brief(&client, &url, &owner, &owner_cookie)
            .json(&invalid)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut spoofed = serde_json::to_value(&request)?;
    spoofed["client"] = json!(worker.view.account_id);
    let response = put_brief(&client, &url, &owner, &owner_cookie)
        .json(&spoofed)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.json::<ApiError>().await?.code, "invalid_request");
    let oversized =
        json!({"expectedRevision":1,"brief":{"summary":"a".repeat(MAX_SIGNABLE_BYTES)}});
    let response = put_brief(&client, &url, &owner, &owner_cookie)
        .json(&oversized)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.json::<ApiError>().await?.code, "request_too_large");
    let missing = format!(
        "{}/api/projects/{}/brief",
        server.url,
        EntityId::from_bytes([88; 16])
    );
    assert_eq!(
        put_brief(&client, &missing, &owner, &owner_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    {
        let provider = fake.lock().await;
        assert_eq!(provider.snapshot.projects, [project]);
        assert!(provider.snapshot.balances.is_empty());
    }
    // Restart the adapter and reapply migrations; stored rows and sessions survive.
    let config = test_config(app.config.database_url.clone(), &internal);
    server.finish().await?;
    app.db.close().await;
    let restarted = Arc::new(App::new(config).await?);
    let server = Server::start(http::router(restarted.clone())).await?;
    let url = format!("{}/api/projects/{project_id}/brief", server.url);
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<ProjectBriefView>()
            .await?,
        saved
    );
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([99; 16]);
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        Value::Null
    );
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    server.finish().await?;
    restarted.db.close().await;
    Ok(())
}

#[tokio::test]
async fn project_briefs_serialize_concurrent_writes_and_retries() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let id = EntityId::from_bytes([42; 16]);
    let instance = info().provider_instance_id;
    let request = brief_request(0, "First")?;
    let second_app = App::new(test_config(app.config.database_url.clone(), &internal)).await?;
    let (a, b) = tokio::join!(
        crate::project_briefs::store::write(&app.db, instance, id, &request),
        crate::project_briefs::store::write(&second_app.db, instance, id, &request)
    );
    let (a, b) = (a?, b?);
    assert_ne!(a.0, b.0);
    assert_eq!(a.1, b.1);
    assert_eq!(a.1.revision, 1);
    let update_a = brief_request(1, "Edit A")?;
    let update_b = brief_request(1, "Edit B")?;
    let (a, b) = tokio::join!(
        crate::project_briefs::store::write(&app.db, instance, id, &update_a),
        crate::project_briefs::store::write(&second_app.db, instance, id, &update_b)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let (winner, loser, winning_request) = match (a, b) {
        (Ok(a), Err(b)) => (a, b, &update_a),
        (Err(a), Ok(b)) => (b, a, &update_b),
        _ => return Err("concurrent edits must have exactly one winner".into()),
    };
    assert!(matches!(loser, Error::Conflict("brief_revision_conflict")));
    assert_eq!(winner.1.revision, 2);
    assert_eq!(
        crate::project_briefs::store::write(&app.db, instance, id, winning_request)
            .await?
            .1,
        winner.1
    );
    assert!(matches!(
        crate::project_briefs::store::write(&app.db, instance, id, &request).await,
        Err(Error::Conflict(_))
    ));
    let missing = EntityId::from_bytes([43; 16]);
    assert!(matches!(
        crate::project_briefs::store::write(&app.db, instance, missing, &update_a).await,
        Err(Error::Conflict(_))
    ));
    app.db.close().await;
    second_app.db.close().await;
    internal.finish().await?;
    Ok(())
}
