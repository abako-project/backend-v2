use super::*;
use crate::proposal_reviews::models::{CommentsPage, PresentationView, ReviewComment};
use serde_json::{Value, json};

fn write(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    session: &Session,
    cookie: &str,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:3000")
        .header("X-CSRF-Token", &session.view.csrf_token)
}
#[tokio::test]
async fn proposal_reviews_persist_versions_comments_and_permissions_without_provider_writes()
-> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, owner_cookie) = register(&app, "review_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "review_coordinator").await?;
    let (_, outsider_cookie) = register(&app, "review_outsider").await?;
    let (worker, worker_cookie) = register(&app, "review_worker").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = owner.view.account_id;
    project.coordinator = coordinator.view.account_id;
    project.planning.status = PlanningStatus::Accepted;
    project.planning.frozen = false;
    project.cancelled = false;
    project.completed = false;
    project.active_dispute_id = None;
    let proposal = &mut project.proposals[0];
    proposal.status = ProposalStatus::Draft;
    let assignment = serde_json::from_value(
        json!({"requirementKey":proposal.milestones[0].definition.requirements[0].key,"worker":worker.view.account_id}),
    )?;
    proposal.milestones[0].assignments.push(assignment);
    let proposal_id = proposal.proposal_id;
    let revision = proposal.revision;
    let request = json!({"expectedProposalRevision":revision,"expectedPresentationRevision":0,"milestones":[{"key":proposal.milestones[0].definition.key,"description":"Figma outcomes","requirements":[]} ]});
    fake.lock().await.snapshot.projects.push(project.clone());
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/projects/{}/proposals/{proposal_id}/presentation",
        server.url, project.project_id
    );
    let messages = url.replace("/presentation", "/comments");
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
    assert_eq!(
        write(&client, reqwest::Method::PUT, &url, &owner, &owner_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .put(&url)
            .header(header::COOKIE, &coordinator_cookie)
            .json(&request)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let saved = write(
        &client,
        reqwest::Method::PUT,
        &url,
        &coordinator,
        &coordinator_cookie,
    )
    .json(&request)
    .send()
    .await?;
    assert_eq!(saved.status(), StatusCode::CREATED);
    let saved = saved.json::<PresentationView>().await?;
    assert_eq!(saved.revision, 1);
    assert!(saved.matches_current_definition);
    let retry = write(
        &client,
        reqwest::Method::PUT,
        &url,
        &coordinator,
        &coordinator_cookie,
    )
    .json(&request)
    .send()
    .await?;
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(retry.json::<PresentationView>().await?, saved);
    let mut conflict = request.clone();
    conflict["milestones"][0]["description"] = json!("stale edit");
    assert_eq!(
        write(
            &client,
            reqwest::Method::PUT,
            &url,
            &coordinator,
            &coordinator_cookie
        )
        .json(&conflict)
        .send()
        .await?
        .status(),
        StatusCode::CONFLICT
    );
    let (a,b)=tokio::join!(write(&client,reqwest::Method::PUT,&url,&coordinator,&coordinator_cookie).json(&json!({"expectedProposalRevision":revision,"expectedPresentationRevision":1,"milestones":[]})).send(),write(&client,reqwest::Method::PUT,&url,&coordinator,&coordinator_cookie).json(&json!({"expectedProposalRevision":revision,"expectedPresentationRevision":1,"milestones":[{"key":project.proposals[0].milestones[0].definition.key,"description":"other concurrent edit","requirements":[]}]})).send());
    let (a, b) = (a?.status(), b?.status());
    assert!(a == StatusCode::CONFLICT || b == StatusCode::CONFLICT);
    assert!(a == StatusCode::OK || b == StatusCode::OK);
    assert_eq!(
        client
            .get(&messages)
            .header(header::COOKIE, &worker_cookie)
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let message = json!({"expectedProposalRevision":revision,"requestId":EntityId::from_bytes([22;16]),"message":"Please clarify the deliverable"});
    let (a, b) = tokio::join!(
        write(
            &client,
            reqwest::Method::POST,
            &messages,
            &owner,
            &owner_cookie
        )
        .json(&message)
        .send(),
        write(
            &client,
            reqwest::Method::POST,
            &messages,
            &owner,
            &owner_cookie
        )
        .json(&message)
        .send()
    );
    let (a, b) = (a?, b?);
    assert_ne!(a.status(), b.status());
    let (a, b) = (
        a.json::<ReviewComment>().await?,
        b.json::<ReviewComment>().await?,
    );
    assert_eq!(a, b);
    assert_eq!(a.author_account, owner.view.account_id);
    assert_eq!(a.definition.title, project.proposals[0].title);
    let mut different = message.clone();
    different["message"] = json!("different retry");
    assert_eq!(
        write(
            &client,
            reqwest::Method::POST,
            &messages,
            &owner,
            &owner_cookie
        )
        .json(&different)
        .send()
        .await?
        .status(),
        StatusCode::CONFLICT
    );
    for i in 1..22 {
        let next = json!({"expectedProposalRevision":revision,"requestId":EntityId::from_bytes([i;16]),"message":format!("Review round {i}")});
        assert_eq!(
            write(
                &client,
                reqwest::Method::POST,
                &messages,
                &coordinator,
                &coordinator_cookie
            )
            .json(&next)
            .send()
            .await?
            .status(),
            StatusCode::CREATED
        );
    }
    let page = client
        .get(&messages)
        .header(header::COOKIE, &owner_cookie)
        .send()
        .await?
        .json::<CommentsPage>()
        .await?;
    assert_eq!(page.items.len(), 20);
    let after = page.next_after.ok_or("missing page cursor")?;
    let second = client
        .get(format!("{messages}?after={after}"))
        .header(header::COOKIE, &owner_cookie)
        .send()
        .await?
        .json::<CommentsPage>()
        .await?;
    assert_eq!(second.items.len(), 2);
    assert!(second.next_after.is_none());
    assert_eq!(
        client
            .get(format!("{messages}?after=-1"))
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    {
        let provider = fake.lock().await;
        assert_eq!(provider.snapshot.projects, [project.clone()]);
        assert_eq!(provider.submissions, 0);
        assert!(provider.snapshot.balances.is_empty());
    }
    {
        let mut provider = fake.lock().await;
        provider.snapshot.projects[0].proposals[0].revision += 1;
        provider.snapshot.projects[0].proposals[0].status = ProposalStatus::PendingApproval;
    }
    assert!(
        client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<PresentationView>()
            .await?
            .matches_current_definition
    );
    let retry = write(
        &client,
        reqwest::Method::POST,
        &messages,
        &owner,
        &owner_cookie,
    )
    .json(&message)
    .send()
    .await?;
    assert_eq!(retry.status(), StatusCode::OK);
    assert_eq!(retry.json::<ReviewComment>().await?, a);
    assert_eq!(write(&client,reqwest::Method::POST,&messages,&owner,&owner_cookie).json(&json!({"expectedProposalRevision":revision,"requestId":EntityId::from_bytes([44;16]),"message":"stale review"})).send().await?.status(),StatusCode::CONFLICT);
    {
        fake.lock().await.snapshot.projects[0].proposals[0].title = "New definition".into();
    }
    assert!(
        !client
            .get(&url)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<PresentationView>()
            .await?
            .matches_current_definition
    );
    let config = test_config(app.config.database_url.clone(), &internal);
    server.finish().await?;
    app.db.close().await;
    let restarted = Arc::new(App::new(config).await?);
    let server = Server::start(http::router(restarted.clone())).await?;
    let messages = format!(
        "{}/api/projects/{}/proposals/{proposal_id}/comments",
        server.url, project.project_id
    );
    assert_eq!(
        client
            .get(&messages)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<CommentsPage>()
            .await?
            .items[0],
        a
    );
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([99; 16]);
    assert!(
        client
            .get(&messages)
            .header(header::COOKIE, &owner_cookie)
            .send()
            .await?
            .json::<CommentsPage>()
            .await?
            .items
            .is_empty()
    );
    internal.finish().await?;
    assert_eq!(
        client
            .get(&messages)
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
