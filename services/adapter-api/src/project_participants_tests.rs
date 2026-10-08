use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn participants_enforce_visibility_and_only_return_public_profiles() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, owner_cookie) = register(&app, "participants_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "participants_coordinator").await?;
    let (_, outsider_cookie) = register(&app, "participants_outsider").await?;
    let (worker, worker_cookie) = register(&app, "participants_worker").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = owner.view.account_id;
    project.coordinator = coordinator.view.account_id;
    let milestone = &mut project.proposals[0].milestones[0];
    let mut definition = doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone();
    definition["assignees"] = json!([worker.view.account_id]);
    milestone.task_storage.tasks.push(serde_json::from_value(json!({"taskId":1,"reporter":worker.view.account_id,"createdAt":1,"updatedAt":1,"task":definition}))?);
    fake.lock().await.snapshot.projects.push(project.clone());
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    for (session, cookie, section, profile) in [
        (
            &owner,
            &owner_cookie,
            "client",
            json!({"name":"Client Name", "company":"Company", "department":"PRIVATE DEPARTMENT", "website":"https://example.test", "description":null, "location":null, "languages":[]}),
        ),
        (
            &coordinator,
            &coordinator_cookie,
            "worker",
            json!({"name":"Coordinator Name", "contactEmail":"PRIVATE@example.test", "githubUsername":null, "portfolioUrl":null, "biography":null, "background":"PRIVATE BACKGROUND", "proficiency":"senior", "location":null, "languages":[]}),
        ),
    ] {
        assert_eq!(
            client
                .put(format!("{}/api/profiles/me", server.url))
                .header(header::COOKIE, cookie)
                .header("X-CSRF-Token", &session.view.csrf_token)
                .header(header::ORIGIN, "http://localhost:3000")
                .json(&json!({"section":section,"profile":profile}))
                .send()
                .await?
                .status(),
            StatusCode::OK
        );
    }
    // The response exposes availability, never image bytes or private profile fields.
    sqlx::query("UPDATE client_profiles SET image_data=$1,image_mime_type='image/png' WHERE principal_id=$2")
        .bind(vec![1_u8,2,3]).bind(owner.view.principal_id.to_string()).execute(&app.db).await?;
    let url = format!(
        "{}/api/projects/{}/participants",
        server.url, project.project_id
    );
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
    for cookie in [&owner_cookie, &coordinator_cookie, &worker_cookie] {
        let response = client
            .get(&url)
            .header(header::COOKIE, cookie)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.json::<Value>().await?;
        assert_eq!(
            body["client"]["accountId"],
            owner.view.account_id.to_string()
        );
        assert_eq!(
            body["client"]["profiles"]["principalId"],
            owner.view.principal_id.to_string()
        );
        assert_eq!(body["client"]["profiles"]["client"]["company"], "Company");
        assert_eq!(body["client"]["clientImage"], true);
        assert_eq!(
            body["coordinator"]["profiles"]["worker"]["name"],
            "Coordinator Name"
        );
        assert_eq!(body["coordinator"]["workerImage"], false);
        let text = body.to_string();
        for private in [
            "PRIVATE",
            "contactEmail",
            "department",
            "background",
            "username",
            "csrfToken",
            "image_data",
        ] {
            assert!(!text.contains(private), "must not expose {private}");
        }
        let schema = &doc["components"]["schemas"]["ProjectParticipantView"];
        for actor in ["client", "coordinator"] {
            let fields = body[actor]
                .as_object()
                .ok_or("participant object missing")?;
            assert_eq!(
                fields.len(),
                schema["required"].as_array().ok_or("schema missing")?.len()
            );
            for key in fields.keys() {
                assert!(schema["properties"].get(key).is_some());
            }
        }
    }
    fake.lock().await.snapshot.projects[0].coordinator = AccountId32::from_bytes([91; 32]);
    let body = client
        .get(&url)
        .header(header::COOKIE, &owner_cookie)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert!(body["coordinator"]["profiles"].is_null());
    assert!(body["coordinator"]["displayName"].is_null());
    assert_eq!(body["coordinator"]["workerImage"], false);
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &coordinator_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    fake.lock().await.snapshot.projects.clear();
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
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
    app.db.close().await;
    Ok(())
}
