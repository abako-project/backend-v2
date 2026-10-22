use super::*;
use parity_scale_codec::Encode;
use serde_json::{Value, json};

async fn queued(
    app: &App,
    session: &Session,
    command: ProviderCommand,
    key: u8,
) -> Result<ProviderCommand, Box<dyn std::error::Error>> {
    let id = OperationId::from_bytes([key; 16]);
    let first = operations::enqueue(app, session, command.clone(), Some(id)).await?;
    assert_eq!(
        operations::enqueue(app, session, command, Some(id)).await?,
        first
    );
    let row = sqlx::query(
        "SELECT command_json,request_command_bytes FROM operations WHERE operation_id=$1",
    )
    .bind(id.to_string())
    .fetch_one(&app.db)
    .await?;
    assert!(
        row.try_get::<Option<Vec<u8>>, _>("request_command_bytes")?
            .is_some()
    );
    Ok(serde_json::from_str(
        &row.try_get::<String, _>("command_json")?,
    )?)
}
fn commitment(
    command: ProviderCommand,
) -> Result<EvidenceCommitmentV1, Box<dyn std::error::Error>> {
    Ok(match command {
        ProviderCommand::SubmitMilestoneCommitted { evidence, .. }
        | ProviderCommand::RejectMilestoneCommitted { evidence, .. }
        | ProviderCommand::OpenDisputeCommitted { evidence, .. }
        | ProviderCommand::RespondDisputeCommitted { evidence, .. } => evidence,
        _ => return Err("expected commitment command".into()),
    })
}

#[tokio::test]
async fn evidence_urls_are_queued_as_hashes_and_public_views_fail_closed() -> TestResult {
    let fake = fake()?;
    let slot = Arc::new(Mutex::new(None::<DisputeView>));
    let case_slot = slot.clone();
    let router = internal(fake.clone()).route(
        "/internal/disputes/{id}",
        get(move |Path(id): Path<String>| {
            let slot = case_slot.clone();
            async move {
                let case = slot.lock().await.clone().ok_or(Error::NotFound)?;
                if case.dispute.dispute_id.to_string() != id {
                    return Err(Error::NotFound);
                }
                Ok::<_, Error>(Json(case))
            }
        }),
    );
    let internal = Server::start(router).await?;
    let app = app(&internal).await?;
    let (client, client_cookie) = register(&app, "evidence_client").await?;
    let (coordinator, _) = register(&app, "evidence_coordinator").await?;
    let (outsider, _) = register(&app, "evidence_outsider").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = client.view.account_id;
    project.coordinator = coordinator.view.account_id;
    let project_id = project.project_id;
    let milestone_id = project.proposals[0].milestones[0].milestone_id;
    let submission_id = EntityId::from_bytes([51; 16]);
    let case_id = EntityId::from_bytes([52; 16]);
    project.proposals[0].milestones[0].submissions = vec![CompletionSubmission {
        submission_id,
        version: 1,
        deliverable: None,
        submitted_by: project.coordinator,
        submitted_at: UnixSeconds::new(100),
        worker_ratings: vec![],
        review: SubmissionReview::PendingReview,
    }];
    let mut case = DisputeView {
        dispute: Dispute {
            dispute_id: case_id,
            project_id,
            milestone_id,
            rejected_submission_id: submission_id,
            status: DisputeStatus::Open,
            opened_by: project.client,
            counterparty: project.coordinator,
            opened_at: UnixSeconds::new(102),
            evidence: None,
            opening_comment_id: None,
            response: None,
            proposal_revision: 1,
            context_event_cursor: 0,
        },
        milestone: project.proposals[0].milestones[0].clone(),
    };
    *slot.lock().await = Some(case.clone());
    fake.lock().await.snapshot.projects.push(project.clone());
    let legacy: EvidenceRecordReferenceV1 = serde_json::from_value(
        json!({"url":"https://example.test/historical","sha256":PayloadHash::from_bytes([8;32])}),
    )?;
    let mut historical = project.clone();
    historical.proposals[0].milestones[0].submissions[0].deliverable = Some(legacy.clone());
    historical.proposals[0].milestones[0].submissions[0].review = SubmissionReview::Rejected {
        reason: legacy.clone(),
        reviewed_by: project.client,
        reviewed_at: UnixSeconds::new(101),
    };
    let expected = historical.clone();
    crate::evidence::project(&app, info().provider_instance_id, &mut historical).await?;
    assert_eq!(historical, expected);
    let mut historical_case = case.dispute.clone();
    historical_case.evidence = Some(legacy.clone());
    historical_case.response = Some(DisputeResponse {
        evidence: legacy,
        author: project.coordinator,
        occurred_at: UnixSeconds::new(103),
    });
    let expected = historical_case.clone();
    let _verified =
        crate::evidence::dispute(&app, info().provider_instance_id, &mut historical_case).await?;
    assert_eq!(historical_case, expected);
    let delivery = queued(
        &app,
        &coordinator,
        ProviderCommand::SubmitMilestoneDelivery {
            project_id,
            milestone_id,
            request: SubmitMilestoneCompletionRequest {
                deliverable: Some(serde_json::from_value(serde_json::json!({
                    "url": "https://example.test/delivery", "sha256": PayloadHash::from_bytes([7; 32])
                }))?),
            },
        },
        61,
    )
    .await?;
    assert!(
        !delivery
            .encode()
            .windows(b"https://".len())
            .any(|bytes| bytes == b"https://")
    );
    let delivery = commitment(delivery)?;
    let reject = commitment(
        queued(
            &app,
            &client,
            ProviderCommand::RejectMilestoneCompletion {
                project_id,
                milestone_id,
                submission_id,
                request: EvidenceRequest {
                    evidence: EvidenceReference::new("https://example.test/rejection".into())?,
                },
            },
            62,
        )
        .await?,
    )?;
    let opening = commitment(
        queued(
            &app,
            &client,
            ProviderCommand::OpenDispute(OpenDisputeRequest {
                project_id,
                milestone_id,
                rejected_submission_id: submission_id,
                evidence: EvidenceReference::new("https://example.test/opening".into())?,
            }),
            63,
        )
        .await?,
    )?;
    let response = commitment(
        queued(
            &app,
            &coordinator,
            ProviderCommand::RespondDispute {
                project_id,
                dispute_id: case_id,
                request: EvidenceRequest {
                    evidence: EvidenceReference::new("https://example.test/response".into())?,
                },
            },
            64,
        )
        .await?,
    )?;
    let changed = ProviderCommand::SubmitMilestoneDelivery {
        project_id,
        milestone_id,
        request: SubmitMilestoneCompletionRequest {
            deliverable: Some(EvidenceReference::new(
                "https://example.test/changed".into(),
            )?),
        },
    };
    assert!(matches!(
        operations::enqueue(
            &app,
            &coordinator,
            changed,
            Some(OperationId::from_bytes([61; 16]))
        )
        .await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    let stored = &mut project.proposals[0].milestones[0].submissions[0];
    stored.deliverable = Some(EvidenceRecordReferenceV1::Committed(delivery.clone()));
    stored.review = SubmissionReview::Rejected {
        reason: EvidenceRecordReferenceV1::Committed(reject),
        reviewed_by: project.client,
        reviewed_at: UnixSeconds::new(101),
    };
    case.milestone = project.proposals[0].milestones[0].clone();
    case.dispute.evidence = Some(EvidenceRecordReferenceV1::Committed(opening));
    case.dispute.response = Some(DisputeResponse {
        evidence: EvidenceRecordReferenceV1::Committed(response.clone()),
        author: project.coordinator,
        occurred_at: UnixSeconds::new(103),
    });
    *slot.lock().await = Some(case);
    let mut unrelated = project.clone();
    unrelated.project_id = EntityId::from_bytes([53; 16]);
    unrelated.client = outsider.view.account_id;
    unrelated.coordinator = outsider.view.account_id;
    fake.lock().await.snapshot.projects = vec![project, unrelated];
    let server = Server::start(http::router(app.clone())).await?;
    let http = reqwest::Client::new();
    let get_project = || {
        http.get(format!("{}/api/projects/{project_id}", server.url))
            .header(header::COOKIE, &client_cookie)
    };
    let read = get_project().send().await?;
    assert_eq!(read.status(), StatusCode::OK);
    let value: Value = read.json().await?;
    let submission = &value["proposals"][0]["milestones"][0]["submissions"][0];
    assert_eq!(
        submission["deliverable"],
        json!({"url":"https://example.test/delivery","sha256":PayloadHash::from_bytes([7;32])})
    );
    assert_eq!(
        submission["review"]["reason"],
        json!({"url":"https://example.test/rejection"})
    );
    assert!(!value.to_string().contains(&delivery.hash.to_string()));
    assert_eq!(
        http.get(format!("{}/api/projects", server.url))
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    let url = format!("{}/api/disputes/{case_id}", server.url);
    let value: Value = http.get(&url).send().await?.json().await?;
    assert_eq!(
        value["dispute"]["evidence"],
        json!({"url":"https://example.test/opening"})
    );
    assert_eq!(
        value["dispute"]["response"]["evidence"],
        json!({"url":"https://example.test/response"})
    );
    sqlx::query("UPDATE evidence_url_records SET payload=$1 WHERE record_id=$2")
        .bind(serde_json::to_string(
            &json!({"url":"https://example.test/tampered"}),
        )?)
        .bind(delivery.record_id.to_string())
        .execute(&app.db)
        .await?;
    assert_eq!(
        get_project().send().await?.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        http.get(format!(
            "{}/api/completion-submissions/{submission_id}/comments",
            server.url
        ))
        .header(header::COOKIE, &client_cookie)
        .send()
        .await?
        .status(),
        StatusCode::OK
    );
    // Authorization and unrelated raw reads must not hydrate the broken record.
    assert_eq!(
        http.get(format!("{}/api/catalog", server.url))
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert!(
        operations::authorize(
            &app,
            &client,
            &ProviderCommand::EvaluateProject {
                project_id,
                request: EvaluateProjectRequest { ratings: vec![] }
            }
        )
        .await
        .is_ok()
    );
    sqlx::query("DELETE FROM evidence_url_records WHERE record_id=$1")
        .bind(response.record_id.to_string())
        .execute(&app.db)
        .await?;
    assert_eq!(
        http.get(&url).send().await?.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Public evidence failure must not change private-channel authorization or
    // block independently stored later arguments in the same confirmed case.
    assert_eq!(
        http.get(format!("{url}/messages"))
            .header(header::COOKIE, &client_cookie)
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("{url}/arguments")).send().await?.status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("{url}/presentation"))
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        http.get(format!("{url}/history")).send().await?.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn evidence_text_hashes_preserve_scoped_comment_ids_and_detect_changes() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (client, _) = register(&app, "evidence_text_client").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = client.view.account_id;
    let project_id = project.project_id;
    let milestone = project.proposals[0].milestones[0].milestone_id;
    fake.lock().await.snapshot.projects.push(project.clone());
    let comment = EntityId::from_bytes([71; 16]);
    let mut submissions = Vec::new();
    for (index, message) in [
        (72, "First reason: ipfs://notes"),
        (73, "Second reason: ordinary explanation"),
    ] {
        let id = EntityId::from_bytes([index; 16]);
        sqlx::query("INSERT INTO submission_comments(provider_instance_id,submission_id,comment_id,author_account,created_at,message,kind) VALUES($1,$2,$3,$4,100,$5,'Rejection')")
            .bind(info().provider_instance_id.to_string()).bind(id.to_string()).bind(comment.to_string()).bind(client.view.account_id.to_string()).bind(message).execute(&app.db).await?;
        let evidence = commitment(
            queued(
                &app,
                &client,
                ProviderCommand::RejectMilestoneDelivery {
                    project_id,
                    milestone_id: milestone,
                    submission_id: id,
                    comment_id: comment,
                },
                index,
            )
            .await?,
        )?;
        assert_eq!(evidence.record_id, comment);
        let submission = CompletionSubmission {
            submission_id: id,
            version: 1,
            deliverable: None,
            submitted_by: project.coordinator,
            submitted_at: UnixSeconds::new(99),
            worker_ratings: vec![],
            review: SubmissionReview::Rejected {
                reason: EvidenceRecordReferenceV1::Committed(evidence),
                reviewed_by: project.client,
                reviewed_at: UnixSeconds::new(100),
            },
        };
        submissions.push(submission);
    }
    let mut first = submissions[0].clone();
    let mut second = submissions[1].clone();
    let first_verified = crate::evidence::submission(
        &app,
        info().provider_instance_id,
        project_id,
        milestone,
        &mut first,
    )
    .await?;
    let _verified = crate::evidence::submission(
        &app,
        info().provider_instance_id,
        project_id,
        milestone,
        &mut second,
    )
    .await?;
    assert!(
        matches!(first.review,SubmissionReview::RejectedWithComment{comment_id,..} if comment_id==comment)
    );
    assert_ne!(submissions[0].review, submissions[1].review);
    sqlx::query("UPDATE submission_comments SET message='Changed reason' WHERE submission_id=$1 AND comment_id=$2").bind(submissions[0].submission_id.to_string()).bind(comment.to_string()).execute(&app.db).await?;
    assert_eq!(
        first_verified.as_deref(),
        Some("First reason: ipfs://notes")
    );
    assert!(matches!(
        crate::evidence::submission(
            &app,
            info().provider_instance_id,
            project_id,
            milestone,
            &mut submissions[0]
        )
        .await,
        Err(Error::Dependency)
    ));
    let _verified = crate::evidence::submission(
        &app,
        info().provider_instance_id,
        project_id,
        milestone,
        &mut submissions[1],
    )
    .await?;
    let opening = EntityId::from_bytes([74; 16]);
    let rejected = submissions[1].submission_id;
    sqlx::query("INSERT INTO dispute_opening_arguments(provider_instance_id,comment_id,project_id,milestone_id,submission_id,author_account,reason) VALUES($1,$2,$3,$4,$5,$6,'Written opening: ipfs://notes')")
        .bind(info().provider_instance_id.to_string()).bind(opening.to_string()).bind(project_id.to_string()).bind(milestone.to_string()).bind(rejected.to_string()).bind(project.client.to_string()).execute(&app.db).await?;
    let evidence = commitment(
        queued(
            &app,
            &client,
            ProviderCommand::OpenDisputeWithComment(OpenDisputeWithCommentRequest {
                project_id,
                milestone_id: milestone,
                rejected_submission_id: rejected,
                comment_id: opening,
            }),
            74,
        )
        .await?,
    )?;
    let mut case = Dispute {
        dispute_id: EntityId::from_bytes([75; 16]),
        project_id,
        milestone_id: milestone,
        rejected_submission_id: rejected,
        status: DisputeStatus::Open,
        opened_by: project.client,
        counterparty: project.coordinator,
        opened_at: UnixSeconds::new(101),
        evidence: Some(EvidenceRecordReferenceV1::Committed(evidence.clone())),
        opening_comment_id: None,
        response: None,
        proposal_revision: 1,
        context_event_cursor: 0,
    };
    let opening_verified =
        crate::evidence::dispute(&app, info().provider_instance_id, &mut case).await?;
    assert_eq!(case.opening_comment_id, Some(opening));
    assert!(case.evidence.is_none());
    sqlx::query("UPDATE dispute_opening_arguments SET reason='Modified' WHERE comment_id=$1")
        .bind(opening.to_string())
        .execute(&app.db)
        .await?;
    assert_eq!(
        opening_verified.as_deref(),
        Some("Written opening: ipfs://notes")
    );
    case.evidence = Some(EvidenceRecordReferenceV1::Committed(evidence));
    case.opening_comment_id = None;
    assert!(matches!(
        crate::evidence::dispute(&app, info().provider_instance_id, &mut case).await,
        Err(Error::Dependency)
    ));
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn evidence_generation_is_bound_before_signing_preparation() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (coordinator, _) = register(&app, "evidence_generation").await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.coordinator = coordinator.view.account_id;
    let id = OperationId::from_bytes([81; 16]);
    let command = ProviderCommand::SubmitMilestoneDelivery {
        project_id: project.project_id,
        milestone_id: project.proposals[0].milestones[0].milestone_id,
        request: SubmitMilestoneCompletionRequest {
            deliverable: Some(EvidenceReference::new(
                "https://example.test/generation".into(),
            )?),
        },
    };
    fake.lock().await.snapshot.projects.push(project);
    operations::enqueue(&app, &coordinator, command, Some(id)).await?;
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([82; 16]);
    let job = operations::claim(&app).await?.ok_or("pending job")?;
    operations::process(&app, job).await?;
    let operation = operations::read(&app, &coordinator, id).await?;
    assert_eq!(operation.status, ProviderOperationStatus::Rejected);
    assert_eq!(
        operation.error_code.as_deref(),
        Some("provider_instance_changed")
    );
    let row=sqlx::query("SELECT provider_instance_id,signable_payload,signed_json FROM operations WHERE operation_id=$1").bind(id.to_string()).fetch_one(&app.db).await?;
    assert_eq!(
        row.try_get::<String, _>("provider_instance_id")?,
        info().provider_instance_id.to_string()
    );
    assert!(
        row.try_get::<Option<Vec<u8>>, _>("signable_payload")?
            .is_none()
    );
    assert!(row.try_get::<Option<String>, _>("signed_json")?.is_none());
    assert!(fake.lock().await.jobs.is_empty());
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
