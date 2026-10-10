use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn dispute_threads_are_immutable_public_or_private_and_survive_restart() -> TestResult {
    let fake = fake()?;
    let (case_slot, case_id) = (
        Arc::new(Mutex::new(None::<DisputeView>)),
        EntityId::from_bytes([61; 16]),
    );
    let slot = case_slot.clone();
    let router = internal(fake.clone()).route(
        "/internal/disputes/{id}",
        get(move |Path(id): Path<String>| {
            let slot = slot.clone();
            async move {
                let c = slot.lock().await.clone().ok_or(Error::NotFound)?;
                if id != c.dispute.dispute_id.to_string() {
                    return Err(Error::NotFound);
                }
                Ok::<_, Error>(Json(c))
            }
        }),
    );
    let internal = Server::start(router).await?;
    let app = app(&internal).await?;
    let (client_session, client_cookie) = register(&app, "case_client").await?;
    let (coordinator, coordinator_cookie) = register(&app, "case_coordinator").await?;
    let (outsider, outsider_cookie) = register(&app, "case_worker").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = client_session.view.account_id;
    project.coordinator = coordinator.view.account_id;
    let (assigned, assigned_cookie) = register(&app, "case_assigned").await?;
    project.proposals[0].status = ProposalStatus::Approved;
    project.proposals[0].milestones[0].assignments = vec![serde_json::from_value(
        json!({"requirementKey": 1, "worker": assigned.view.account_id}),
    )?];
    let submission_id = EntityId::from_bytes([62; 16]);
    let comment_id = EntityId::from_bytes([63; 16]);
    let milestone_id = project.proposals[0].milestones[0].milestone_id;
    let milestone = &mut project.proposals[0].milestones[0];
    milestone.status = Some(MilestoneStatus::ChangesRequested);
    milestone.submissions = vec![CompletionSubmission {
        submission_id,
        version: 1,
        deliverable: None,
        submitted_by: coordinator.view.account_id,
        submitted_at: UnixSeconds::new(100),
        worker_ratings: vec![],
        review: SubmissionReview::RejectedWithComment {
            comment_id,
            reviewed_by: client_session.view.account_id,
            reviewed_at: UnixSeconds::new(101),
        },
    }];
    fake.lock().await.snapshot.projects.push(project.clone());
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let opening_key = EntityId::from_bytes([64; 16]);
    let opening = json!({"projectId":project.project_id,"milestoneId":milestone_id,"rejectedSubmissionId":submission_id,"reason":"Written opening with ipfs://notes and ordinary explanation."});
    let write = |path: String, cookie: &str, csrf: &str, body: &Value| {
        client
            .post(path)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "http://localhost:3000")
            .header("X-CSRF-Token", csrf)
            .json(body)
    };
    let open = |body: &Value| {
        write(
            format!("{}/api/disputes", server.url),
            &client_cookie,
            &client_session.view.csrf_token,
            body,
        )
        .header("Idempotency-Key", opening_key.to_string())
    };
    let response = open(&opening).send().await?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted = response.json::<Value>().await?;
    assert_eq!(
        open(&opening).send().await?.json::<Value>().await?,
        accepted
    );
    let mut changed = opening.clone();
    changed["reason"] = json!("Changed opening");
    assert_eq!(open(&changed).send().await?.status(), StatusCode::CONFLICT);
    let mut forged = opening.clone();
    forged["authorAccount"] = json!(project.client);
    assert_eq!(
        open(&forged).send().await?.status(),
        StatusCode::BAD_REQUEST
    );
    let action: ProviderCommand = serde_json::from_str(
        &sqlx::query("SELECT command_json FROM operations WHERE operation_id=$1")
            .bind(opening_key.to_string())
            .fetch_one(&app.db)
            .await?
            .try_get::<String, _>("command_json")?,
    )?;
    assert!(
        matches!(action,ProviderCommand::OpenDisputeWithComment(ref r) if r.comment_id==opening_key)
    );
    project.active_dispute_id = Some(case_id);
    project.proposals[0].milestones[0].frozen = true;
    project.proposals[0].milestones[0].status = Some(MilestoneStatus::Disputed);
    let case = DisputeView {
        dispute: Dispute {
            dispute_id: case_id,
            project_id: project.project_id,
            milestone_id,
            rejected_submission_id: submission_id,
            status: DisputeStatus::Open,
            opened_by: project.client,
            counterparty: project.coordinator,
            opened_at: UnixSeconds::new(102),
            evidence: None,
            opening_comment_id: Some(opening_key),
            response: None,
            proposal_revision: project.proposals[0].revision,
            context_event_cursor: 0,
        },
        milestone: project.proposals[0].milestones[0].clone(),
    };
    *case_slot.lock().await = Some(case);
    fake.lock().await.snapshot.projects = vec![project.clone()];
    sqlx::query("INSERT INTO submission_comments(provider_instance_id,submission_id,comment_id,author_account,created_at,message,kind) VALUES($1,$2,$3,$4,101,'Correct the keyboard navigation.','Rejection')").bind(info().provider_instance_id.to_string()).bind(submission_id.to_string()).bind(comment_id.to_string()).bind(project.client.to_string()).execute(&app.db).await?;
    sqlx::query("INSERT INTO submission_comments(provider_instance_id,submission_id,comment_id,author_account,created_at,message,kind) VALUES($1,$2,$3,$4,101,'PRIVATE DELIVERY DISCUSSION','Comment')").bind(info().provider_instance_id.to_string()).bind(submission_id.to_string()).bind(EntityId::from_bytes([65;16]).to_string()).bind(project.coordinator.to_string()).execute(&app.db).await?;
    sqlx::query("INSERT INTO submission_presentations(provider_instance_id,submission_id,documentation,links) VALUES($1,$2,'ipfs://docs','sftp://server/notes')").bind(info().provider_instance_id.to_string()).bind(submission_id.to_string()).execute(&app.db).await?;
    let base = format!("{}/api/disputes/{case_id}", server.url);
    let public = client.get(format!("{base}/presentation")).send().await?;
    assert_eq!(public.status(), StatusCode::OK);
    let public = public.json::<Value>().await?;
    assert_eq!(public["presentation"]["openingReason"], opening["reason"]);
    assert_eq!(
        public["presentation"]["client"]["accountId"],
        json!(project.client)
    );
    assert!(!public.to_string().contains("csrfToken"));
    assert!(!public.to_string().contains("contactEmail"));
    let history = client
        .get(format!("{base}/history"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(history["deliverables"][0]["documentation"], "ipfs://docs");
    assert_eq!(
        history["rejections"][0]["reason"],
        "Correct the keyboard navigation."
    );
    assert!(!history.to_string().contains("PRIVATE DELIVERY DISCUSSION"));
    let entry_id = EntityId::from_bytes([66; 16]);
    let argument = json!({"entryId":entry_id,"content":"First response in plain text","argumentType":"RESPONSE"});
    let post = |body: &Value| {
        write(
            format!("{base}/arguments"),
            &client_cookie,
            &client_session.view.csrf_token,
            body,
        )
    };
    assert_eq!(
        client
            .post(format!("{base}/arguments"))
            .json(&argument)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        write(
            format!("{base}/arguments"),
            &outsider_cookie,
            &outsider.view.csrf_token,
            &argument
        )
        .send()
        .await?
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(
            format!("{base}/arguments"),
            &client_cookie,
            "wrong",
            &argument
        )
        .send()
        .await?
        .status(),
        StatusCode::FORBIDDEN
    );
    let first = post(&argument).send().await?;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = first.json::<Value>().await?;
    assert_eq!(first["author"]["accountId"], json!(project.client));
    assert_eq!(post(&argument).send().await?.json::<Value>().await?, first);
    let mut changed = argument.clone();
    changed["content"] = json!("Replacement");
    assert_eq!(post(&changed).send().await?.status(), StatusCode::CONFLICT);
    let mut blank = argument.clone();
    blank["content"] = json!(" ");
    assert_eq!(post(&blank).send().await?.status(), StatusCode::BAD_REQUEST);
    let mut forged = argument.clone();
    forged["author"] = json!(project.coordinator);
    assert_eq!(
        post(&forged).send().await?.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for n in 0..22u8 {
        let body = json!({"entryId":EntityId::from_bytes([70+n;16]),"content":format!("Coordinator reply {n}"),"argumentType":if n%2==0 {"RESPONSE"} else {"ADDITIONAL"}});
        assert_eq!(
            write(
                format!("{base}/arguments"),
                &coordinator_cookie,
                &coordinator.view.csrf_token,
                &body
            )
            .send()
            .await?
            .status(),
            StatusCode::CREATED
        );
    }
    let page = client
        .get(format!("{base}/arguments"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(page["items"].as_array().ok_or("items")?.len(), 20);
    let after = page["nextAfter"].as_i64().ok_or("cursor")?;
    let last = client
        .get(format!("{base}/arguments?after={after}"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(last["items"].as_array().ok_or("items")?.len(), 3);
    assert!(last["nextAfter"].is_null());
    let message = json!({"entryId":EntityId::from_bytes([99;16]),"content":"PRIVATE DISPUTE CONVERSATION","argumentType":"MESSAGE"});
    assert_eq!(
        post(&message).send().await?.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        write(
            format!("{base}/messages"),
            &coordinator_cookie,
            &coordinator.view.csrf_token,
            &message
        )
        .send()
        .await?
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        client
            .get(format!("{base}/messages"))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/messages"))
            .header(header::COOKIE, &outsider_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    let private = client
        .get(format!("{base}/messages"))
        .header(header::COOKIE, &client_cookie)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(private["items"][0]["content"], message["content"]);
    assert_eq!(private["canWrite"], false);
    let coordinator_page = client
        .get(format!("{base}/messages"))
        .header(header::COOKIE, &coordinator_cookie)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(coordinator_page["canWrite"], true);
    assert_eq!(
        write(
            format!("{base}/messages"),
            &client_cookie,
            &client_session.view.csrf_token,
            &message
        )
        .send()
        .await?
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{base}/messages"))
            .header(header::COOKIE, &assigned_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    let history = client
        .get(format!("{base}/history"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    let activity: Vec<_> = history["events"]
        .as_array()
        .ok_or("events")?
        .iter()
        .filter(|e| e["eventType"] == "DisputeArgumentAdded")
        .collect();
    assert_eq!(activity.len(), 23);
    assert_eq!(activity[0]["eventData"]["argumentId"], json!(entry_id));
    assert!(!history.to_string().contains("PRIVATE DISPUTE CONVERSATION"));
    let mut expanded_config = test_config(app.config.database_url.clone(), &internal);
    expanded_config.dispute_channel_allow_participants = true;
    let expanded = Arc::new(App::new(expanded_config).await?);
    let expanded_server = Server::start(http::router(expanded.clone())).await?;
    let expanded_base = format!("{}/api/disputes/{case_id}", expanded_server.url);
    for (session, cookie, number) in [
        (&client_session, &client_cookie, 110),
        (&assigned, &assigned_cookie, 111),
    ] {
        let page = client
            .get(format!("{expanded_base}/messages"))
            .header(header::COOKIE, cookie)
            .send()
            .await?
            .json::<Value>()
            .await?;
        assert_eq!(page["canWrite"], true);
        let body = json!({"entryId":EntityId::from_bytes([number;16]),"content":"PARTICIPANT PRIVATE MESSAGE","argumentType":"MESSAGE"});
        assert_eq!(
            write(
                format!("{expanded_base}/messages"),
                cookie,
                &session.view.csrf_token,
                &body
            )
            .send()
            .await?
            .status(),
            StatusCode::CREATED
        );
    }
    assert_eq!(
        client
            .get(format!("{expanded_base}/messages"))
            .header(header::COOKIE, &outsider_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(
            format!("{expanded_base}/arguments"),
            &assigned_cookie,
            &assigned.view.csrf_token,
            &argument
        )
        .send()
        .await?
        .status(),
        StatusCode::NOT_FOUND
    );
    expanded_server.finish().await?;
    expanded.db.close().await;
    let private = client
        .get(format!("{base}/messages"))
        .header(header::COOKIE, &client_cookie)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(private["canWrite"], false);
    assert_eq!(private["items"][2]["author"]["role"], "worker");
    assert_eq!(
        client
            .get(format!("{base}/messages"))
            .header(header::COOKIE, &assigned_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );

    assert!(
        !client
            .get(format!("{base}/arguments"))
            .send()
            .await?
            .text()
            .await?
            .contains("PRIVATE DISPUTE CONVERSATION")
    );
    assert_eq!(
        client
            .put(format!("{base}/arguments"))
            .header(header::COOKIE, &client_cookie)
            .header(header::ORIGIN, "http://localhost:3000")
            .header("X-CSRF-Token", &client_session.view.csrf_token)
            .json(&changed)
            .send()
            .await?
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        client
            .delete(format!("{base}/messages"))
            .header(header::COOKIE, &client_cookie)
            .header(header::ORIGIN, "http://localhost:3000")
            .header("X-CSRF-Token", &client_session.view.csrf_token)
            .send()
            .await?
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(fake.lock().await.snapshot.projects, [project]);
    assert_eq!(fake.lock().await.submissions, 0);
    server.finish().await?;
    let restarted =
        Arc::new(App::new(test_config(app.config.database_url.clone(), &internal)).await?);
    let server = Server::start(http::router(restarted.clone())).await?;
    assert_eq!(
        client
            .get(format!("{}/api/disputes/{case_id}/messages", server.url))
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        private
    );
    server.finish().await?;
    internal.finish().await?;
    restarted.db.close().await;
    app.db.close().await;
    Ok(())
}
