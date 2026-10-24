//! Public task views in explicit API-owned fixtures; no activation procedure.
use super::*;
use generated_contracts::provider_core::{CoreDisputeView, ProjectCore};
use serde_json::json;

async fn get(
    server: &Server,
    cookie: &str,
    path: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    reqwest::Client::new()
        .get(format!("{}{path}", server.url))
        .header(header::COOKIE, cookie)
        .send()
        .await
}
async fn project(c: &Cohort, account: AccountId32) -> Result<Option<ProjectCore>, Error> {
    let raw = c.fake.lock().await.snapshot.projects[0].clone();
    Ok(
        crate::task_views::project(&c.app, info().provider_instance_id, raw, account)
            .await?
            .0,
    )
}

#[tokio::test]
async fn api_task_views_grant_and_remove_task_only_access_without_r1_copies() -> TestResult {
    let c = Cohort::new(true).await?;
    let server = Server::start(http::router(c.app.clone())).await?;
    let path = format!("/api/projects/{}", c.project.project_id);
    assert_eq!(
        get(&server, &c.worker_cookie, &path).await?.status(),
        StatusCode::NOT_FOUND
    );
    c.run(&c.owner, c.create()).await?;
    for suffix in [
        String::new(),
        "/brief".into(),
        "/participants".into(),
        format!(
            "/proposals/{}/presentation",
            c.project.proposals[0].proposal_id
        ),
    ] {
        assert_eq!(
            get(&server, &c.worker_cookie, &format!("{path}{suffix}"))
                .await?
                .status(),
            StatusCode::OK
        );
    }
    let view = get(&server, &c.worker_cookie, &path)
        .await?
        .json::<ProjectView>()
        .await?;
    let task = &view.proposals[0].milestones[0].task_storage.tasks[0];
    assert_eq!(task.task, c.definition);
    assert_eq!(task.task_id, 9);
    assert_eq!(view.client, c.project.client);
    assert_eq!(view.execution_escrow, c.project.execution_escrow);
    let list = get(&server, &c.worker_cookie, "/api/projects")
        .await?
        .json::<Vec<ProjectView>>()
        .await?;
    assert_eq!(list.len(), 1);
    let storage_path = format!("/api/task-storages/{}", c.storage());
    assert_eq!(
        get(&server, &c.worker_cookie, &storage_path)
            .await?
            .json::<TaskStorageView>()
            .await?
            .tasks[0],
        *task
    );
    assert_eq!(
        get(
            &server,
            &c.worker_cookie,
            &format!("{storage_path}/tasks/9")
        )
        .await?
        .json::<TaskView>()
        .await?,
        *task
    );
    let participants = get(&server, &c.worker_cookie, &format!("{path}/participants"))
        .await?
        .json::<Value>()
        .await?;
    assert!(
        participants["workers"]
            .as_array()
            .ok_or("workers")?
            .is_empty()
    );
    let mut edit = c.definition.clone();
    edit.assignees.clear();
    c.run(
        &c.owner,
        ProviderCommand::EditTask {
            project_id: c.project.project_id,
            task_storage_id: c.storage(),
            task_id: 9,
            task: edit,
        },
    )
    .await?;
    assert_eq!(
        get(&server, &c.worker_cookie, &path).await?.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        get(&server, &c.worker_cookie, "/api/projects")
            .await?
            .json::<Vec<ProjectView>>()
            .await?
            .is_empty()
    );
    assert_eq!(
        get(&server, &c.worker_cookie, &storage_path)
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    let history = notifications::page(&c.app, c.worker.view.account_id, 0).await?;
    assert_eq!(history.notifications.len(), 1);
    assert!(history.notifications[0].project_title.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_content_copies")
            .fetch_one(&c.app.db)
            .await?,
        0
    );
    assert!(
        c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
            .task_storage
            .tasks
            .is_empty()
    );
    server.finish().await?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_task_progress_uses_current_membership_but_money_authorization_stays_raw() -> TestResult
{
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let command = ProviderCommand::UpdateTaskProgress {
        project_id: c.project.project_id,
        task_storage_id: c.storage(),
        task_id: 9,
        progress: TaskProgressRequest {
            status: TaskStatus::Done,
            logged_minutes: Minutes::new(2),
        },
    };
    operations::authorize(&c.app, &c.owner, &command).await?;
    operations::authorize(&c.app, &c.worker, &command).await?;
    assert!(matches!(
        operations::authorize(&c.app, &c.client, &command).await,
        Err(Error::Forbidden)
    ));
    sqlx::query("UPDATE task_items SET payload='not JSON'")
        .execute(&c.app.db)
        .await?;
    let approval = ProviderCommand::ApproveExecution {
        project_id: c.project.project_id,
        proposal_id: c.project.proposals[0].proposal_id,
        expected_revision: 1,
    };
    operations::authorize(&c.app, &c.client, &approval).await?;
    assert!(matches!(
        operations::authorize(&c.app, &c.owner, &command).await,
        Err(Error::Dependency)
    ));
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_task_view_corruption_isolated_from_unrelated_projects_and_durable_history()
-> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let mut private = c.fake.lock().await.snapshot.projects[0].clone();
    private.project_id = EntityId::from_bytes([31; 16]);
    private.client = AccountId32::from_bytes([32; 32]);
    private.coordinator = AccountId32::from_bytes([33; 32]);
    private.proposals[0].proposal_id = EntityId::from_bytes([34; 16]);
    private.proposals[0].milestones[0].milestone_id = EntityId::from_bytes([35; 16]);
    private.proposals[0].milestones[0].task_storage.milestone_id = EntityId::from_bytes([35; 16]);
    private.proposals[0].milestones[0]
        .task_storage
        .task_storage_id = EntityId::from_bytes([36; 16]);
    let storage = private.proposals[0].milestones[0].task_storage.clone();
    sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$3,$4,'1',$5)")
        .bind(info().provider_instance_id.to_string()).bind(storage.task_storage_id.to_string()).bind(private.project_id.to_string()).bind(private.proposals[0].proposal_id.to_string()).bind(storage.milestone_id.to_string()).execute(&c.app.db).await?;
    sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,1,$2,0,'not JSON',$3)")
        .bind(info().provider_instance_id.to_string()).bind(storage.task_storage_id.to_string()).bind(vec![private.coordinator.to_string()]).execute(&c.app.db).await?;
    c.fake.lock().await.snapshot.projects.push(private.clone());
    let server = Server::start(http::router(c.app.clone())).await?;
    assert_eq!(
        get(
            &server,
            &c.owner_cookie,
            &format!("/api/projects/{}", c.project.project_id)
        )
        .await?
        .status(),
        StatusCode::OK
    );
    let list = get(&server, &c.owner_cookie, "/api/projects")
        .await?
        .json::<Vec<ProjectView>>()
        .await?;
    assert_eq!(list.len(), 1);
    // An outsider cannot distinguish corrupt private data through a task read.
    assert_eq!(
        get(
            &server,
            &c.owner_cookie,
            &format!("/api/projects/{}", private.project_id)
        )
        .await?
        .status(),
        StatusCode::NOT_FOUND
    );
    // A corrupt positive membership projection still cannot grant private access.
    sqlx::query("UPDATE task_items SET assignees=$1 WHERE task_id=1")
        .bind(vec![c.owner.view.account_id.to_string()])
        .execute(&c.app.db)
        .await?;
    assert_eq!(
        get(
            &server,
            &c.owner_cookie,
            &format!("/api/projects/{}", private.project_id)
        )
        .await?
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    sqlx::query("UPDATE task_items SET payload='not JSON' WHERE task_id=9")
        .execute(&c.app.db)
        .await?;
    let history = notifications::page(&c.app, c.worker.view.account_id, 0).await?;
    assert_eq!(history.notifications.len(), 1);
    assert!(history.notifications[0].project_title.is_none());
    server.finish().await?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_task_views_ignore_stale_provider_tasks_and_validate_storage_context() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let original = c.tasks().await?[0].clone();
    c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
        .task_storage
        .tasks = vec![TaskView {
        task_id: 1,
        task: TaskDefinition {
            assignees: vec![c.client.view.account_id],
            ..c.definition.clone()
        },
        ..original.clone()
    }];
    assert_eq!(
        project(&c, c.worker.view.account_id)
            .await?
            .ok_or("worker access")?
            .proposals[0]
            .milestones[0]
            .task_storage
            .tasks,
        vec![original]
    );
    let stranger = AccountId32::from_bytes([37; 32]);
    c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
        .task_storage
        .tasks[0]
        .task
        .assignees = vec![stranger];
    assert!(project(&c, stranger).await?.is_none());
    sqlx::query("UPDATE task_storages SET milestone_id=$1")
        .bind(EntityId::from_bytes([38; 16]).to_string())
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        project(&c, c.owner.view.account_id).await,
        Err(Error::Dependency)
    ));
    c.internal.finish().await?;
    Ok(())
}

async fn blocked_task_reader(c: &Cohort) -> Result<(), Error> {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid WHERE a.wait_event_type='Lock' AND l.relation='task_items'::regclass AND l.database=(SELECT oid FROM pg_database WHERE datname=current_database()))")
                .fetch_one(&c.app.db).await?;
            if waiting{return Ok::<_,Error>(());}tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.map_err(|_|Error::Internal)?
}
#[tokio::test]
async fn api_task_views_keep_one_pg_snapshot_during_an_atomic_task_commit() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let mut next = c.tasks().await?[0].clone();
    next.task_id = 10;
    let mut publisher = c.app.db.begin().await?;
    sqlx::query("LOCK TABLE task_items IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *publisher)
        .await?;
    let read = project(&c, c.owner.view.account_id);
    let publish = async {
        let blocked = blocked_task_reader(&c).await;
        if blocked.is_ok() {
            sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,10,$2,1,$3,$4)")
                .bind(info().provider_instance_id.to_string()).bind(c.storage().to_string()).bind(serde_json::to_string(&next)?)
                .bind(next.task.assignees.iter().map(ToString::to_string).collect::<Vec<_>>()).execute(&mut *publisher).await?;
            sqlx::query("UPDATE task_generations SET next_task_id=11,task_count=task_count+1")
                .execute(&mut *publisher)
                .await?;
        }
        publisher.commit().await?;
        blocked
    };
    let (view, committed) = tokio::join!(read, publish);
    committed?;
    assert_eq!(
        view?.ok_or("visible")?.proposals[0].milestones[0]
            .task_storage
            .tasks
            .len(),
        1
    );
    assert_eq!(
        project(&c, c.owner.view.account_id)
            .await?
            .ok_or("visible")?
            .proposals[0]
            .milestones[0]
            .task_storage
            .tasks
            .len(),
        2
    );
    c.internal.finish().await?;
    Ok(())
}

async fn case_fixture()
-> Result<(Cohort, CaseSlot, EntityId, OperationId), Box<dyn std::error::Error>> {
    let slot = Arc::new(Mutex::new(None));
    let c = Cohort::with_case(true, Some(slot.clone())).await?;
    c.run(&c.owner, c.create()).await?;
    let case_id = EntityId::from_bytes([41; 16]);
    let submission = EntityId::from_bytes([42; 16]);
    {
        let mut fake = c.fake.lock().await;
        let core = &mut fake.snapshot.projects[0];
        core.proposals[0].status = ProposalStatus::Approved;
        let milestone = &mut core.proposals[0].milestones[0];
        milestone.status = Some(MilestoneStatus::ChangesRequested);
        milestone.submissions = vec![CompletionSubmission {
            submission_id: submission,
            version: 1,
            deliverable: None,
            submitted_by: c.owner.view.account_id,
            submitted_at: UnixSeconds::new(100),
            worker_ratings: vec![],
            review: SubmissionReview::RejectedWithComment {
                comment_id: EntityId::from_bytes([43; 16]),
                reviewed_by: c.client.view.account_id,
                reviewed_at: UnixSeconds::new(101),
            },
        }];
        fake.next_entity = Some(case_id);
    }
    let request = ProviderCommand::OpenDispute(OpenDisputeRequest {
        project_id: c.project.project_id,
        milestone_id: c.project.proposals[0].milestones[0].milestone_id,
        rejected_submission_id: submission,
        evidence: EvidenceReference::new("https://example.test/opening".into())?,
    });
    let id = operations::enqueue(&c.app, &c.owner, request, None)
        .await?
        .operation_id;
    let result = drive(&c.app, &c.owner, id).await?;
    let receipt = result.receipt.ok_or("receipt")?;
    let command: ProviderCommand = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT command_json FROM operations WHERE operation_id=$1",
        )
        .bind(id.to_string())
        .fetch_one(&c.app.db)
        .await?,
    )?;
    let ProviderCommand::OpenDisputeCommitted { evidence, .. } = command else {
        return Err("committed opening".into());
    };
    let mut fake = c.fake.lock().await;
    let core = &mut fake.snapshot.projects[0];
    core.active_dispute_id = Some(case_id);
    let milestone = &mut core.proposals[0].milestones[0];
    milestone.status = Some(MilestoneStatus::Disputed);
    milestone.frozen = true;
    let view = CoreDisputeView {
        dispute: Dispute {
            dispute_id: case_id,
            project_id: c.project.project_id,
            milestone_id: milestone.milestone_id,
            rejected_submission_id: submission,
            status: DisputeStatus::Open,
            opened_by: c.owner.view.account_id,
            counterparty: c.client.view.account_id,
            opened_at: receipt.finalized_at,
            evidence: Some(EvidenceRecordReferenceV1::Committed(evidence)),
            opening_comment_id: None,
            response: None,
            proposal_revision: 1,
            context_event_cursor: 0,
        },
        milestone: milestone.clone(),
    };
    drop(fake);
    *slot.lock().await = Some(view);
    Ok((c, slot, case_id, id))
}

#[tokio::test]
async fn api_frozen_views_use_the_accepted_snapshot_and_keep_private_access_contractual()
-> TestResult {
    let (c, _, id, _) = case_fixture().await?;
    let before = crate::disputes::public_read(&c.app, id).await?;
    assert_eq!(before.milestone.task_storage.tasks, c.tasks().await?);
    // Distinct current-state fixture proves the frozen read never substitutes it.
    let mut current = c.tasks().await?[0].clone();
    current.task.title = "Different current state".into();
    sqlx::query("UPDATE task_items SET payload=$1")
        .bind(serde_json::to_string(&current)?)
        .execute(&c.app.db)
        .await?;
    assert_eq!(crate::disputes::public_read(&c.app, id).await?, before);
    let mut config = test_config(c.app.config.database_url.clone(), &c.internal)?;
    config.dispute_channel_allow_participants = true;
    let app = Arc::new(App::new(config).await?);
    let server = Server::start(http::router(app)).await?;
    let path = format!("/api/disputes/{id}");
    assert_eq!(
        get(&server, &c.worker_cookie, &path).await?.status(),
        StatusCode::OK
    );
    assert_eq!(
        get(&server, &c.worker_cookie, &format!("{path}/messages"))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&server, &c.client_cookie, &format!("{path}/messages"))
            .await?
            .status(),
        StatusCode::OK
    );
    server.finish().await?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_frozen_views_reject_receipt_and_snapshot_context_mismatches() -> TestResult {
    let (c, slot, id, operation) = case_fixture().await?;
    let receipt: String =
        sqlx::query_scalar("SELECT receipt_json FROM operations WHERE operation_id=$1")
            .bind(operation.to_string())
            .fetch_one(&c.app.db)
            .await?;
    let original: Value = serde_json::from_str(&receipt)?;
    for (field, value) in [
        ("nonce", json!(99)),
        ("createdEntityId", json!(EntityId::from_bytes([51; 16]))),
        ("origin", json!(c.client.view.account_id)),
        (
            "providerInstanceId",
            json!(ProviderInstanceId::from_bytes([52; 16])),
        ),
        ("operationId", json!(OperationId::from_bytes([53; 16]))),
        ("finalizedAt", json!(0)),
        ("outcome", json!({"type":"Failed","code":"failed"})),
    ] {
        let mut altered = original.clone();
        altered[field] = value;
        sqlx::query("UPDATE operations SET receipt_json=$1 WHERE operation_id=$2")
            .bind(serde_json::to_string(&altered)?)
            .bind(operation.to_string())
            .execute(&c.app.db)
            .await?;
        assert!(
            matches!(
                crate::disputes::public_read(&c.app, id).await,
                Err(Error::Dependency)
            ),
            "field {field}"
        );
    }
    sqlx::query("UPDATE operations SET receipt_json=$1 WHERE operation_id=$2")
        .bind(receipt)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    let signed: String =
        sqlx::query_scalar("SELECT signed_json FROM operations WHERE operation_id=$1")
            .bind(operation.to_string())
            .fetch_one(&c.app.db)
            .await?;
    let mut altered: Value = serde_json::from_str(&signed)?;
    altered["call"]["nonce"] = json!(99);
    sqlx::query("UPDATE operations SET signed_json=$1 WHERE operation_id=$2")
        .bind(serde_json::to_string(&altered)?)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    sqlx::query("UPDATE operations SET signed_json=$1 WHERE operation_id=$2")
        .bind(signed)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    let bytes: Vec<u8> =
        sqlx::query_scalar("SELECT command_bytes FROM operations WHERE operation_id=$1")
            .bind(operation.to_string())
            .fetch_one(&c.app.db)
            .await?;
    sqlx::query("UPDATE operations SET command_bytes=$1 WHERE operation_id=$2")
        .bind(vec![255u8])
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    sqlx::query("UPDATE operations SET command_bytes=$1 WHERE operation_id=$2")
        .bind(bytes)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    let source: String =
        sqlx::query_scalar("SELECT snapshot_json FROM task_freezes WHERE operation_id=$1")
            .bind(operation.to_string())
            .fetch_one(&c.app.db)
            .await?;
    let original: Value = serde_json::from_str(&source)?;
    for (field, value) in [
        ("projectId", json!(EntityId::from_bytes([54; 16]))),
        ("proposalId", json!(EntityId::from_bytes([55; 16]))),
        ("proposalRevision", json!(99)),
    ] {
        let mut altered = original.clone();
        altered[0][field] = value;
        sqlx::query("UPDATE task_freezes SET snapshot_json=$1 WHERE operation_id=$2")
            .bind(serde_json::to_string(&altered)?)
            .bind(operation.to_string())
            .execute(&c.app.db)
            .await?;
        assert!(matches!(
            crate::disputes::public_read(&c.app, id).await,
            Err(Error::Dependency)
        ));
    }
    sqlx::query("UPDATE task_freezes SET snapshot_json=$1 WHERE operation_id=$2")
        .bind(source)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    for state in ["Pending", "Released"] {
        sqlx::query("UPDATE task_freezes SET state=$1")
            .bind(state)
            .execute(&c.app.db)
            .await?;
        assert!(matches!(
            crate::disputes::public_read(&c.app, id).await,
            Err(Error::Dependency)
        ));
    }
    sqlx::query("UPDATE task_freezes SET state='Confirmed'")
        .execute(&c.app.db)
        .await?;
    slot.lock().await.as_mut().ok_or("case")?.milestone.frozen = false;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_frozen_views_fail_closed_for_unreconciled_cases_and_preserve_provider_mode()
-> TestResult {
    let (c, _, id, _) = case_fixture().await?;
    sqlx::query("DELETE FROM task_freezes")
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    sqlx::query("UPDATE task_generations SET api_owned=FALSE")
        .execute(&c.app.db)
        .await?;
    assert!(
        crate::disputes::public_read(&c.app, id)
            .await?
            .milestone
            .task_storage
            .tasks
            .is_empty()
    );
    assert!(project(&c, c.worker.view.account_id).await?.is_none());
    let server = Server::start(http::router(c.app.clone())).await?;
    assert_eq!(
        get(
            &server,
            &c.owner_cookie,
            &format!("/api/projects/{}", c.project.project_id)
        )
        .await?
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_content_copies")
            .fetch_one(&c.app.db)
            .await?,
        1
    );
    server.finish().await?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_task_view_namespace_and_new_empty_storage_reads_do_not_reuse_old_tasks() -> TestResult
{
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let generation = ProviderInstanceId::from_bytes([61; 16]);
    // Explicit candidate namespace fixture, not a cutover or a source fingerprint.
    sqlx::query("INSERT INTO task_generations(provider_instance_id,next_task_id,source_hash,storage_count,task_count,api_owned) VALUES($1,99,$2,0,0,TRUE)")
        .bind(generation.to_string()).bind(vec![0u8;32]).execute(&c.app.db).await?;
    let core = c.fake.lock().await.snapshot.projects[0].clone();
    let (other, owned) =
        crate::task_views::project(&c.app, generation, core, c.owner.view.account_id).await?;
    assert!(owned);
    assert!(
        other.ok_or("owner")?.proposals[0].milestones[0]
            .task_storage
            .tasks
            .is_empty()
    );
    assert_eq!(
        project(&c, c.owner.view.account_id)
            .await?
            .ok_or("owner")?
            .proposals[0]
            .milestones[0]
            .task_storage
            .tasks
            .len(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM task_storages WHERE provider_instance_id=$1"
        )
        .bind(generation.to_string())
        .fetch_one(&c.app.db)
        .await?,
        0
    );
    // A nonempty provider storage with no verified API context cannot fall back.
    let mut core = c.fake.lock().await.snapshot.projects[0].clone();
    core.proposals[0].milestones[0].task_storage.tasks = c.tasks().await?;
    assert!(matches!(
        crate::task_views::project(&c.app, generation, core, c.owner.view.account_id).await,
        Err(Error::Dependency)
    ));
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn api_frozen_view_rejects_ambiguous_openings_and_storage_key_corruption() -> TestResult {
    let (c, _, id, operation) = case_fixture().await?;
    let duplicate = OperationId::from_bytes([62; 16]);
    // An ambiguous retained graph must fail even before validating its payloads.
    sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,signable_payload,signed_json,receipt_json,possibly_submitted,request_command_bytes) SELECT $1,principal_id,wallet_id,account_id,command_json,command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,signable_payload,signed_json,receipt_json,possibly_submitted,request_command_bytes FROM operations WHERE operation_id=$2")
        .bind(duplicate.to_string()).bind(operation.to_string()).execute(&c.app.db).await?;
    sqlx::query("INSERT INTO task_freezes(operation_id,provider_instance_id,project_id,kind,state,snapshot_json) SELECT $1,provider_instance_id,project_id,kind,state,snapshot_json FROM task_freezes WHERE operation_id=$2")
        .bind(duplicate.to_string()).bind(operation.to_string()).execute(&c.app.db).await?;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    sqlx::query("DELETE FROM task_freezes WHERE operation_id=$1")
        .bind(duplicate.to_string())
        .execute(&c.app.db)
        .await?;
    sqlx::query("DELETE FROM operations WHERE operation_id=$1")
        .bind(duplicate.to_string())
        .execute(&c.app.db)
        .await?;
    let original: String =
        sqlx::query_scalar("SELECT snapshot_json FROM task_freezes WHERE operation_id=$1")
            .bind(operation.to_string())
            .fetch_one(&c.app.db)
            .await?;
    for (field, value) in [
        ("taskStorageId", json!(EntityId::from_bytes([63; 16]))),
        ("milestoneId", json!(EntityId::from_bytes([64; 16]))),
    ] {
        let mut snapshot: Value = serde_json::from_str(&original)?;
        snapshot[0]["storage"][field] = value;
        sqlx::query("UPDATE task_freezes SET snapshot_json=$1 WHERE operation_id=$2")
            .bind(serde_json::to_string(&snapshot)?)
            .bind(operation.to_string())
            .execute(&c.app.db)
            .await?;
        assert!(matches!(
            crate::disputes::public_read(&c.app, id).await,
            Err(Error::Dependency)
        ));
    }
    let mut snapshot: Value = serde_json::from_str(&original)?;
    snapshot[0]["storage"]["tasks"][0]["taskId"] = json!(u32::MAX);
    sqlx::query("UPDATE task_freezes SET snapshot_json=$1 WHERE operation_id=$2")
        .bind(serde_json::to_string(&snapshot)?)
        .bind(operation.to_string())
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        crate::disputes::public_read(&c.app, id).await,
        Err(Error::Dependency)
    ));
    c.internal.finish().await?;
    Ok(())
}
