use super::*;
use generated_contracts::provider_core::*;
use serde_json::Value;
fn definition() -> Result<ProposalDefinition, ContractError> {
    let week = Week::new(2026, 40)?;
    Ok(ProposalDefinition {
        title: "Cafe\u{301}\0 proposal".into(),
        description: "λ".repeat(40_000),
        milestones: vec![MilestoneDefinition {
            key: 1,
            title: "Milestone\0title".into(),
            window: WeekWindow::new(week, week)?,
            coordinator_fee: Money::new(10),
            coordinator_minutes: Minutes::new(30),
            requirements: vec![RequirementDefinition {
                key: 1,
                role_id: 2,
                skill_ids: vec![1],
                minutes: Minutes::new(60),
                budget: Money::new(90),
            }],
        }],
    })
}
async fn empty_project(
    app: &App,
    fake: &Arc<Mutex<Fake>>,
    account: AccountId32,
) -> Result<ProjectView, Box<dyn std::error::Error>> {
    let mut project = content::source_project()?;
    project.coordinator = account;
    project.proposals.clear();
    fake.lock().await.snapshot.projects =
        vec![stage_project(app, info().provider_instance_id, project.clone()).await?];
    Ok(project)
}
#[test]
fn proposal_sha256_vectors_preserve_exact_text_and_context() -> TestResult {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../specs/0015-adapter-core-boundary/proposal-metadata-vectors.json"
    ))?;
    for vector in fixture["vectors"].as_array().ok_or("vectors")? {
        let record: ProposalMetadataRecordV1 = serde_json::from_value(vector["record"].clone())?;
        let digest = PayloadHash::from_bytes(hash(&record.hash_input()));
        assert_eq!(digest.to_string(), vector["sha256"]);
        let mut changed = record.clone();
        changed.generation = ProviderInstanceId::from_bytes([42; 16]);
        assert_ne!(hash(&changed.hash_input()), digest.into_bytes());
        let mut changed = record.clone();
        changed.project_id = EntityId::from_bytes([42; 16]);
        assert_ne!(hash(&changed.hash_input()), digest.into_bytes());
        if let ProposalContentV1::Definition(definition) = &record.content {
            let mut changed = record.clone();
            let ProposalContentV1::Definition(other) = &mut changed.content else {
                return Err("definition".into());
            };
            other.title = definition.title.replace("e\u{301}", "é");
            assert_ne!(hash(&changed.hash_input()), digest.into_bytes());
        }
    }
    Ok(())
}
#[tokio::test]
async fn proposal_staging_is_compact_pending_private_and_uses_captured_core_policy() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, cookie) = register(&app, "proposal_owner").await?;
    let (_, stranger) = register(&app, "proposal_outsider").await?;
    let mut project = empty_project(&app, &fake, owner.view.account_id).await?;
    fake.lock().await.snapshot.catalog.compensation_policy = CompensationPolicy {
        coordinator_percent: Percentage::new(70)?,
    };
    let request = ProviderCommand::CreateProposal {
        project_id: project.project_id,
        proposal: definition()?,
    };
    let queued = operations::enqueue(&app, &owner, request.clone(), None).await?;
    let command_json: String =
        sqlx::query_scalar("SELECT command_json FROM operations WHERE operation_id=$1")
            .bind(queued.operation_id.to_string())
            .fetch_one(&app.db)
            .await?;
    let ProviderCommand::CreateProposalCore {
        content, proposal, ..
    } = serde_json::from_str(&command_json)?
    else {
        return Err("compact command".into());
    };
    assert_eq!(proposal.milestones[0].coordinator_fee, Money::new(10));
    assert_eq!(
        proposal.milestones[0].requirements[0].budget,
        Money::new(90)
    );
    assert!(!command_json.contains("λ"));
    assert!(!command_json.contains("title"));
    let payload: String = sqlx::query_scalar("SELECT payload FROM proposal_documents")
        .fetch_one(&app.db)
        .await?;
    let record: ProposalMetadataRecordV1 = serde_json::from_str(&payload)?;
    assert_eq!(
        record,
        ProposalMetadataRecordV1::from_command(
            info().provider_instance_id,
            owner.view.account_id,
            queued.operation_id,
            &request
        )
        .ok_or("record")?
    );
    let server = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/api/projects/{}", server.url, project.project_id);
    assert!(
        client
            .get(&url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .json::<ProjectView>()
            .await?
            .proposals
            .is_empty()
    );
    // Confirmed facts capture 20%, irrespective of both the input and current 70%.
    let mut confirmed = content::source_project()?;
    confirmed.client = project.client;
    confirmed.coordinator = project.coordinator;
    confirmed.proposals[0].revision = 0;
    confirmed.proposals[0].status = ProposalStatus::Draft;
    confirmed.proposals[0].title = definition()?.title;
    confirmed.proposals[0].description = definition()?.description;
    confirmed.proposals[0].change_request = None;
    confirmed.proposals[0].compensation_policy = Some(CompensationPolicy {
        coordinator_percent: Percentage::new(20)?,
    });
    confirmed.proposals[0].milestones.truncate(1);
    let milestone = &mut confirmed.proposals[0].milestones[0];
    milestone.definition = definition()?.milestones.remove(0);
    milestone.definition.coordinator_fee = Money::new(20);
    milestone.definition.requirements[0].budget = Money::new(80);
    milestone.status = None;
    milestone.assignments.clear();
    milestone.worker_ratings.clear();
    milestone.submissions.clear();
    milestone.task_storage.tasks.clear();
    project.proposals = confirmed.proposals;
    let mut core = core_project(&project, info().provider_instance_id)?.0;
    core.proposals[0].content = ProposalContentReferenceV1::Signed(content);
    fake.lock().await.snapshot.projects = vec![core];
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .json::<ProjectView>()
            .await?,
        project
    );
    assert_eq!(
        operations::enqueue(&app, &owner, request, Some(queued.operation_id)).await?,
        queued
    );
    sqlx::query("UPDATE proposal_documents SET payload='tampered'")
        .execute(&app.db)
        .await?;
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &stranger)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let storage = &project.proposals[0].milestones[0].task_storage;
    assert_eq!(
        client
            .get(format!(
                "{}/api/task-storages/{}",
                server.url, storage.task_storage_id
            ))
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .json::<TaskStorageView>()
            .await?,
        *storage
    );
    operations::enqueue(
        &app,
        &owner,
        ProviderCommand::CancelProject {
            project_id: project.project_id,
            request: ReasonRequest {
                reason: "Core remains writable".into(),
            },
        },
        None,
    )
    .await?;
    assert!(fake.lock().await.jobs.is_empty());
    assert_eq!(fake.lock().await.submissions, 0);
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
#[tokio::test]
async fn proposal_enqueue_rolls_back_both_sides_and_retries_without_losing_content() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, _) = register(&app, "proposal_atomic").await?;
    let project = empty_project(&app, &fake, owner.view.account_id).await?;
    let request = ProviderCommand::CreateProposal {
        project_id: project.project_id,
        proposal: definition()?,
    };
    let op = OperationId::from_bytes([91; 16]);
    sqlx::raw_sql("CREATE FUNCTION reject_operation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END $$; CREATE TRIGGER deny_operation BEFORE INSERT ON operations FOR EACH ROW EXECUTE FUNCTION reject_operation()").execute(&app.db).await?;
    assert!(
        operations::enqueue(&app, &owner, request.clone(), Some(op))
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM proposal_documents")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::raw_sql("DROP TRIGGER deny_operation ON operations; DROP FUNCTION reject_operation()")
        .execute(&app.db)
        .await?;
    let accepted = operations::enqueue(&app, &owner, request.clone(), Some(op)).await?;
    assert_eq!(
        operations::enqueue(&app, &owner, request.clone(), Some(op)).await?,
        accepted
    );
    let mut changed = definition()?;
    changed.description.push_str("changed");
    assert!(matches!(
        operations::enqueue(
            &app,
            &owner,
            ProviderCommand::CreateProposal {
                project_id: project.project_id,
                proposal: changed
            },
            Some(op)
        )
        .await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    sqlx::raw_sql("CREATE FUNCTION reject_document() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END $$; CREATE TRIGGER deny_document BEFORE INSERT ON proposal_documents FOR EACH ROW EXECUTE FUNCTION reject_document()").execute(&app.db).await?;
    assert!(
        operations::enqueue(
            &app,
            &owner,
            request.clone(),
            Some(OperationId::from_bytes([92; 16]))
        )
        .await
        .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM proposal_documents")
            .fetch_one(&app.db)
            .await?,
        1
    );
    sqlx::raw_sql(
        "DROP TRIGGER deny_document ON proposal_documents; DROP FUNCTION reject_document()",
    )
    .execute(&app.db)
    .await?;
    operations::enqueue(
        &app,
        &owner,
        request,
        Some(OperationId::from_bytes([92; 16])),
    )
    .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM proposal_documents")
            .fetch_one(&app.db)
            .await?,
        2
    );
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
#[tokio::test]
async fn proposal_generation_and_occupied_hash_fail_before_signing() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, _) = register(&app, "proposal_generation").await?;
    let project = empty_project(&app, &fake, owner.view.account_id).await?;
    let command = ProviderCommand::CreateProposal {
        project_id: project.project_id,
        proposal: definition()?,
    };
    let op = OperationId::from_bytes([93; 16]);
    let record = ProposalMetadataRecordV1::from_command(
        info().provider_instance_id,
        owner.view.account_id,
        op,
        &command,
    )
    .ok_or("record")?;
    sqlx::query("INSERT INTO proposal_documents(provider_instance_id,content_hash,payload) VALUES($1,$2,'tampered')").bind(record.generation.to_string()).bind(hash(&record.hash_input()).as_slice()).execute(&app.db).await?;
    assert!(matches!(
        operations::enqueue(&app, &owner, command.clone(), Some(op)).await,
        Err(Error::Dependency)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::query("DELETE FROM proposal_documents")
        .execute(&app.db)
        .await?;
    let op = operations::enqueue(&app, &owner, command, None)
        .await?
        .operation_id;
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([99; 16]);
    let job = operations::claim(&app).await?.ok_or("queued proposal")?;
    operations::process(&app, job).await?;
    let result = operations::read(&app, &owner, op).await?;
    assert_eq!(result.status, ProviderOperationStatus::Rejected);
    assert_eq!(
        result.error_code.as_deref(),
        Some("provider_instance_changed")
    );
    assert!(fake.lock().await.jobs.is_empty());
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
