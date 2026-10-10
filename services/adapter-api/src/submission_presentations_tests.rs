use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn submission_links_are_private_immutable_retryable_and_persistent() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (client_session, client_cookie) = register(&app, "delivery_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "delivery_coordinator").await?;
    let (_, outsider_cookie) = register(&app, "delivery_outsider").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = client_session.view.account_id;
    project.coordinator = coordinator.view.account_id;
    let submission_id = EntityId::from_bytes([42; 16]);
    project.proposals[0].milestones[0].submissions = vec![CompletionSubmission {
        submission_id,
        version: 1,
        deliverable: None,
        submitted_by: coordinator.view.account_id,
        submitted_at: UnixSeconds::new(100),
        worker_ratings: vec![],
        review: SubmissionReview::PendingReview,
    }];
    fake.lock().await.snapshot.projects.push(project.clone());
    let server = Server::start(http::router(app.clone())).await?;
    let url = format!(
        "{}/api/completion-submissions/{submission_id}/presentation",
        server.url
    );
    let client = reqwest::Client::new();
    let body = json!({"expectedRevision":0,"documentation":"ipfs://bafyexample/docs","links":"sftp://example.test/notes"});
    let write = |cookie: &str, csrf: &str, body: &Value| {
        client
            .put(&url)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "http://localhost:3000")
            .header("X-CSRF-Token", csrf)
            .json(body)
    };
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
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        Value::Null
    );
    assert_eq!(
        write(&client_cookie, &client_session.view.csrf_token, &body)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .put(&url)
            .header(header::COOKIE, &coordinator_cookie)
            .json(&body)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut invalid = body.clone();
    invalid["links"] = json!("é".repeat(1025));
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, &invalid)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    invalid = body.clone();
    invalid["authorAccount"] = json!(project.client);
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, &invalid)
            .send()
            .await?
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = write(&coordinator_cookie, &coordinator.view.csrf_token, &body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let saved = response.json::<Value>().await?;
    assert_eq!(saved["submissionId"], json!(submission_id));
    assert_eq!(saved["documentation"], json!("ipfs://bafyexample/docs"));
    assert_eq!(saved["links"], json!("sftp://example.test/notes"));
    let (a, b) = tokio::join!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, &body).send(),
        write(&coordinator_cookie, &coordinator.view.csrf_token, &body).send()
    );
    assert_eq!(a?.status(), StatusCode::OK);
    assert_eq!(b?.status(), StatusCode::OK);
    invalid = body.clone();
    invalid["links"] = json!("https://example.test/changed");
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, &invalid)
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        saved
    );
    assert_eq!(fake.lock().await.snapshot.projects, [project]);
    assert_eq!(fake.lock().await.submissions, 0);
    server.finish().await?;
    let restarted =
        Arc::new(App::new(test_config(app.config.database_url.clone(), &internal)).await?);
    let server = Server::start(http::router(restarted.clone())).await?;
    let url = format!(
        "{}/api/completion-submissions/{submission_id}/presentation",
        server.url
    );
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        saved
    );
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([99; 16]);
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        Value::Null
    );
    server.finish().await?;
    internal.finish().await?;
    restarted.db.close().await;
    app.db.close().await;
    Ok(())
}
