use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn delivery_comments_verify_authors_and_hide_unconfirmed_rejections() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (client_session, client_cookie) = register(&app, "comment_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "comment_coordinator").await?;
    let (outsider, outsider_cookie) = register(&app, "comment_outsider").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = client_session.view.account_id;
    project.coordinator = coordinator.view.account_id;
    let submission_id = EntityId::from_bytes([42; 16]);
    let reason_id = EntityId::from_bytes([43; 16]);
    project.proposals[0].milestones[0].submissions = vec![CompletionSubmission {
        submission_id,
        version: 1,
        deliverable: None,
        submitted_by: coordinator.view.account_id,
        submitted_at: UnixSeconds::new(100),
        worker_ratings: vec![],
        review: SubmissionReview::PendingReview,
    }];
    let project_id = project.project_id;
    let milestone_id = project.proposals[0].milestones[0].milestone_id;
    fake.lock().await.snapshot.projects.push(project);
    let rejection = ProviderCommand::RejectMilestoneDelivery {
        project_id,
        milestone_id,
        submission_id,
        comment_id: reason_id,
    };
    operations::authorize(&app, &client_session, &rejection).await?;
    assert!(matches!(
        operations::authorize(&app, &coordinator, &rejection).await,
        Err(Error::Forbidden)
    ));
    let evaluation = ProviderCommand::EvaluateProject {
        project_id,
        request: EvaluateProjectRequest { ratings: vec![] },
    };
    operations::authorize(&app, &client_session, &evaluation).await?;
    operations::authorize(&app, &coordinator, &evaluation).await?;
    assert!(matches!(
        operations::authorize(&app, &outsider, &evaluation).await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        operations::authorize(&app, &outsider, &rejection).await,
        Err(Error::Forbidden)
    ));
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/completion-submissions/{submission_id}/comments",
        server.url
    );
    let write = |cookie: &str, csrf: &str, body: Value| {
        client
            .post(&url)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "http://localhost:3000")
            .header("X-CSRF-Token", csrf)
            .json(&body)
    };
    let reason = json!({"commentId":reason_id,"message":"Fix the navigation","kind":"Rejection"});
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
        write(
            &coordinator_cookie,
            &coordinator.view.csrf_token,
            reason.clone()
        )
        .send()
        .await?
        .status(),
        StatusCode::FORBIDDEN
    );
    let stored = write(
        &client_cookie,
        &client_session.view.csrf_token,
        reason.clone(),
    )
    .send()
    .await?;
    assert_eq!(stored.status(), StatusCode::CREATED);
    let record: Value = stored.json().await?;
    assert_eq!(
        record["authorAccount"],
        json!(client_session.view.account_id)
    );
    assert_eq!(
        write(
            &client_cookie,
            &client_session.view.csrf_token,
            reason.clone()
        )
        .send()
        .await?
        .status(),
        StatusCode::OK
    );
    let hidden: Value = client
        .get(&url)
        .header(header::COOKIE, &coordinator_cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(hidden["items"], json!([]));
    fake.lock().await.snapshot.projects[0].proposals[0].milestones[0].submissions[0].review =
        SubmissionReview::RejectedWithComment {
            comment_id: reason_id,
            reviewed_by: client_session.view.account_id,
            reviewed_at: UnixSeconds::new(101),
        };
    let shown: Value = client
        .get(&url)
        .header(header::COOKIE, &coordinator_cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(shown["items"][0], record);
    let reply = json!({"commentId":EntityId::from_bytes([44;16]),"message":"Navigation corrected","kind":"Comment"});
    assert_eq!(
        write(
            &coordinator_cookie,
            &coordinator.view.csrf_token,
            reply.clone()
        )
        .send()
        .await?
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        write(
            &coordinator_cookie,
            &coordinator.view.csrf_token,
            reply.clone()
        )
        .send()
        .await?
        .status(),
        StatusCode::OK
    );
    let mut wrong = reply.clone();
    wrong["message"] = json!("Changed payload");
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, wrong)
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    wrong = reply.clone();
    wrong["authorAccount"] = json!(client_session.view.account_id);
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, wrong)
            .send()
            .await?
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let shown: Value = client
        .get(&url)
        .header(header::COOKIE, &client_cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(shown["items"].as_array().ok_or("comments")?.len(), 2);
    fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
        .submissions
        .push(CompletionSubmission {
            submission_id: EntityId::from_bytes([45; 16]),
            version: 2,
            deliverable: None,
            submitted_by: coordinator.view.account_id,
            submitted_at: UnixSeconds::new(102),
            worker_ratings: vec![],
            review: SubmissionReview::PendingReview,
        });
    let fresh =
        json!({"commentId":EntityId::from_bytes([46;16]),"message":"Stale reply","kind":"Comment"});
    assert_eq!(
        write(&coordinator_cookie, &coordinator.view.csrf_token, fresh)
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    let history: Value = client
        .get(&url)
        .header(header::COOKIE, &client_cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(history, shown);
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
