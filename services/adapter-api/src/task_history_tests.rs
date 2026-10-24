//! Seeded future histories test the reader bridge, not API task execution.
use super::*;
use parity_scale_codec::Encode;
use serde_json::{Value, json};

fn task_command() -> Result<ProviderCommand, Box<dyn std::error::Error>> {
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    let mut task: TaskDefinition = serde_json::from_value(
        doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone(),
    )?;
    task.description = "λ\0".repeat(20_000);
    task.assignees.clear();
    Ok(ProviderCommand::CreateTask {
        project_id: project.project_id,
        task_storage_id: project.proposals[0].milestones[0]
            .task_storage
            .task_storage_id,
        task,
    })
}

async fn retained_operation(
    app: &App,
    session: &Session,
    id: OperationId,
    status: &str,
) -> Result<(ProviderCommand, TaskOperationResult, AdapterTaskEvent), Box<dyn std::error::Error>> {
    let command = task_command()?;
    let ProviderCommand::CreateTask {
        project_id,
        task_storage_id,
        ..
    } = &command
    else {
        return Err("wrong fixture command".into());
    };
    let result = TaskOperationResult {
        outcome: ExecutionOutcome::Success,
        task_storage_id: *task_storage_id,
        finalized_at: UnixSeconds::new(1000),
    };
    let event = AdapterTaskEvent {
        source: TaskEventSource::AdapterTask,
        provider_instance_id: info().provider_instance_id,
        operation_id: id,
        kind: TaskEventKind::Created,
        project_id: *project_id,
        task_storage_id: *task_storage_id,
        recipients: vec![session.view.account_id],
        occurred_at: result.finalized_at,
        origin: session.view.account_id,
    };
    sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,request_command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,task_operation,task_result_json) VALUES($1,$2,$3,$4,$5,$6,$6,$7,0,0,1,$8,TRUE,$9)")
        .bind(id.to_string()).bind(session.view.principal_id.to_string()).bind(session.wallet.to_string())
        .bind(session.view.account_id.to_string()).bind(serde_json::to_string(&command)?).bind(command.encode())
        .bind(status).bind(info().provider_instance_id.to_string())
        .bind((status=="Finalized").then(||serde_json::to_string(&result)).transpose()?)
        .execute(&app.db).await?;
    Ok((command, result, event))
}

async fn retained_notification(app: &App, event: &AdapterTaskEvent) -> Result<i64, Error> {
    let mut tx = app.db.begin().await?;
    // A fixture follows the documented future-writer protocol. No production
    // API task writer is introduced by this helper.
    sqlx::query("SELECT pg_advisory_xact_lock(621008)")
        .execute(&mut *tx)
        .await?;
    let id = sqlx::query_scalar("INSERT INTO notifications(account_id,provider_instance_id,api_operation_id,event_json) VALUES($1,$2,$3,$4) RETURNING notification_id")
        .bind(event.recipients[0].to_string()).bind(event.provider_instance_id.to_string())
        .bind(event.operation_id.to_string()).bind(serde_json::to_string(event)?)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

#[tokio::test]
async fn retained_task_results_reopen_and_replay_both_routes_without_signing() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (alice, cookie) = register(&app, "task_history_alice").await?;
    let (bob, _) = register(&app, "task_history_bob").await?;
    let id = OperationId::from_bytes([61; 16]);
    let (command, result, event) = retained_operation(&app, &alice, id, "Finalized").await?;
    let notification_id = retained_notification(&app, &event).await?;
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.coordinator = alice.view.account_id;
    project.client = bob.view.account_id;
    let core = stage_project(&app, info().provider_instance_id, project).await?;
    fake.lock().await.snapshot.projects.push(core);
    let database_url = app.config.database_url.clone();
    drop(app);
    let reopened = Arc::new(App::new(test_config(database_url, &internal)?).await?);
    let actual = operations::read(&reopened, &alice, id).await?;
    assert_eq!(actual.status, ProviderOperationStatus::Finalized);
    assert_eq!(actual.task_result, Some(result));
    assert!(actual.receipt.is_none());
    assert!(matches!(
        operations::read(&reopened, &bob, id).await,
        Err(Error::NotFound)
    ));
    let server = Server::start(http::router(reopened.clone())).await?;
    let client = reqwest::Client::new();
    let ProviderCommand::CreateTask {
        project_id,
        task_storage_id,
        task,
    } = command
    else {
        return Err("wrong fixture command".into());
    };
    for path in [
        format!("/api/projects/{project_id}/task-storages/{task_storage_id}/tasks"),
        format!("/api/task-storages/{task_storage_id}/tasks"),
    ] {
        let response = client
            .post(format!("{}{path}", server.url))
            .header(header::COOKIE, &cookie)
            .header(header::ORIGIN, "http://localhost:8088")
            .header("X-CSRF-Token", &alice.view.csrf_token)
            .header("Idempotency-Key", id.to_string())
            .json(&task)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            response.json::<OperationRef>().await?.status,
            ProviderOperationStatus::Finalized
        );
    }
    let mut changed = task;
    changed.title.push_str(" conflicting retry");
    assert_eq!(
        client
            .post(format!(
                "{}/api/projects/{project_id}/task-storages/{task_storage_id}/tasks",
                server.url
            ))
            .header(header::COOKIE, &cookie)
            .header(header::ORIGIN, "http://localhost:8088")
            .header("X-CSRF-Token", &alice.view.csrf_token)
            .header("Idempotency-Key", id.to_string())
            .json(&changed)
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert!(fake.lock().await.jobs.is_empty());
    assert!(fake.lock().await.receipts.is_empty());
    internal.finish().await?;
    // Reopen and historical reads do not require a live provider generation.
    assert_eq!(operations::read(&reopened, &alice, id).await?, actual);
    let page = notifications::page(&reopened, alice.view.account_id, 0).await?;
    assert_eq!(page.notifications.len(), 1);
    assert_eq!(page.notifications[0].event, NotificationEvent::Task(event));
    assert!(page.notifications[0].actor.is_some());
    assert!(page.notifications[0].read_at.is_none());
    assert!(
        notifications::page(&reopened, bob.view.account_id, 0)
            .await?
            .notifications
            .is_empty()
    );
    let marked = notifications::mark_read(
        &reopened,
        alice.view.account_id,
        u64::try_from(notification_id)?,
    )
    .await?;
    assert!(marked.read_at.is_some());
    assert_eq!(
        notifications::mark_read(
            &reopened,
            alice.view.account_id,
            u64::try_from(notification_id)?
        )
        .await?,
        marked
    );
    assert!(matches!(
        notifications::mark_read(
            &reopened,
            bob.view.account_id,
            u64::try_from(notification_id)?
        )
        .await,
        Err(Error::NotFound)
    ));
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn retained_task_queue_never_enters_signer_or_overtakes_wallet_fifo() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "task_queue_alice").await?;
    let (bob, _) = register(&app, "task_queue_bob").await?;
    let id = OperationId::from_bytes([62; 16]);
    retained_operation(&app, &alice, id, "Queued").await?;
    let later = operations::enqueue(&app, &alice, create(), None).await?;
    assert!(operations::claim(&app).await?.is_none());
    let other = operations::enqueue(&app, &bob, create(), None).await?;
    assert!(operations::claim(&app).await?.is_some());
    let rows =
        sqlx::query("SELECT operation_id,lease_token FROM operations ORDER BY creation_sequence")
            .fetch_all(&app.db)
            .await?;
    for row in rows {
        let operation: String = row.try_get("operation_id")?;
        assert_eq!(
            row.try_get::<Option<String>, _>("lease_token")?.is_some(),
            operation == other.operation_id.to_string()
        );
    }
    let pending = operations::read(&app, &alice, id).await?;
    assert_eq!(pending.status, ProviderOperationStatus::Queued);
    assert!(pending.receipt.is_none() && pending.task_result.is_none());
    assert_eq!(
        operations::read(&app, &alice, later.operation_id)
            .await?
            .status,
        ProviderOperationStatus::AwaitingSignature
    );
    assert!(fake.lock().await.jobs.is_empty());
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn stale_provider_job_refuses_api_owned_work_before_signing() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "task_stale_alice").await?;
    let reference = operations::enqueue(&app, &alice, create(), None).await?;
    let job = operations::claim(&app).await?.ok_or("job missing")?;
    // Simulate an unsupported in-place ownership change after dispatch. No
    // real migration may convert in-flight work this way.
    sqlx::query("UPDATE operations SET task_operation=TRUE,status='Queued',provider_instance_id=$1,command_json=$2,command_bytes=$3 WHERE operation_id=$4")
        .bind(info().provider_instance_id.to_string()).bind(serde_json::to_string(&task_command()?)?)
        .bind(task_command()?.encode()).bind(reference.operation_id.to_string()).execute(&app.db).await?;
    assert!(matches!(
        operations::process(&app, job).await,
        Err(Error::Dependency)
    ));
    assert!(fake.lock().await.jobs.is_empty());
    assert!(fake.lock().await.receipts.is_empty());
    assert_eq!(
        operations::read(&app, &alice, reference.operation_id)
            .await?
            .status,
        ProviderOperationStatus::Queued
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn mixed_notification_history_keeps_one_sse_cursor_and_provider_cursor() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, cookie) = register(&app, "task_sse_alice").await?;
    let (_, _, event) =
        retained_operation(&app, &alice, OperationId::from_bytes([63; 16]), "Finalized").await?;
    let provider = |cursor| DomainEvent {
        provider_instance_id: info().provider_instance_id,
        cursor,
        operation_id: OperationId::from_bytes([91; 16]),
        kind: DomainEventKind::TaskUpdated,
        project_id: None,
        entity_id: None,
        recipients: vec![alice.view.account_id],
        occurred_at: UnixSeconds::new(1),
        origin: None,
    };
    notifications::persist(
        &app,
        0,
        ProviderEvents {
            provider_instance_id: info().provider_instance_id,
            events: vec![provider(1)],
            next_cursor: 1,
        },
    )
    .await?;
    let task_id = retained_notification(&app, &event).await?;
    notifications::persist(
        &app,
        1,
        ProviderEvents {
            provider_instance_id: info().provider_instance_id,
            events: vec![provider(2)],
            next_cursor: 2,
        },
    )
    .await?;
    let page = notifications::page(&app, alice.view.account_id, 0).await?;
    assert_eq!(page.notifications.len(), 3);
    assert!(matches!(
        page.notifications[0].event,
        NotificationEvent::Provider(_)
    ));
    assert!(matches!(
        page.notifications[1].event,
        NotificationEvent::Task(_)
    ));
    assert!(matches!(
        page.notifications[2].event,
        NotificationEvent::Provider(_)
    ));
    assert!(
        page.notifications
            .windows(2)
            .all(|w| w[0].notification_id < w[1].notification_id)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT cursor FROM event_cursors")
            .fetch_one(&app.db)
            .await?,
        2
    );
    let server = Server::start(http::router(app.clone())).await?;
    let mut stream = reqwest::Client::new()
        .get(format!("{}/api/events?after=0", server.url))
        .header(header::COOKIE, &cookie)
        .header(
            "Last-Event-ID",
            page.notifications[0].notification_id.to_string(),
        )
        .send()
        .await?;
    assert_eq!(stream.status(), StatusCode::OK);
    let chunk = tokio::time::timeout(Duration::from_secs(5), stream.chunk())
        .await??
        .ok_or("SSE data missing")?;
    let body = std::str::from_utf8(&chunk)?;
    assert!(body.contains(&format!("id: {task_id}\n")));
    assert!(body.contains("\"source\":\"AdapterTask\""));
    assert!(
        notifications::page(&app, alice.view.account_id, u64::try_from(task_id)?)
            .await?
            .notifications
            .len()
            == 1
    );
    drop(stream);
    server.finish().await?;
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_history_rejects_mismatched_context_and_rolls_back_read_mark() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "task_binding_alice").await?;
    let id = OperationId::from_bytes([64; 16]);
    let (_, result, event) = retained_operation(&app, &alice, id, "Finalized").await?;
    let notification = retained_notification(&app, &event).await?;
    let original = serde_json::to_value(&event)?;
    for (field, value) in [
        (
            "providerInstanceId",
            json!(ProviderInstanceId::from_bytes([92; 16])),
        ),
        ("operationId", json!(OperationId::from_bytes([92; 16]))),
        ("projectId", json!(EntityId::from_bytes([92; 16]))),
        ("taskStorageId", json!(EntityId::from_bytes([92; 16]))),
        ("origin", json!(AccountId32::from_bytes([92; 32]))),
        ("kind", json!("TaskDeleted")),
        ("occurredAt", json!(1001)),
        ("recipients", json!([])),
        ("source", json!("Unknown")),
        ("cursor", json!(1)),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        sqlx::query("UPDATE notifications SET event_json=$1 WHERE notification_id=$2")
            .bind(serde_json::to_string(&changed)?)
            .bind(notification)
            .execute(&app.db)
            .await?;
        assert!(
            notifications::page(&app, alice.view.account_id, 0)
                .await
                .is_err(),
            "accepted changed {field}"
        );
        assert!(
            notifications::mark_read(&app, alice.view.account_id, u64::try_from(notification)?)
                .await
                .is_err()
        );
        assert!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT read_at FROM notifications WHERE notification_id=$1"
            )
            .bind(notification)
            .fetch_one(&app.db)
            .await?
            .is_none()
        );
    }
    sqlx::query("UPDATE notifications SET event_json=$1 WHERE notification_id=$2")
        .bind(serde_json::to_string(&event)?)
        .bind(notification)
        .execute(&app.db)
        .await?;
    let mut failed = result.clone();
    failed.outcome = ExecutionOutcome::Failed("project_frozen".into());
    sqlx::query("UPDATE operations SET task_result_json=$1 WHERE operation_id=$2")
        .bind(serde_json::to_string(&failed)?)
        .bind(id.to_string())
        .execute(&app.db)
        .await?;
    assert_eq!(
        operations::read(&app, &alice, id).await?.task_result,
        Some(failed)
    );
    assert!(
        notifications::page(&app, alice.view.account_id, 0)
            .await
            .is_err()
    );
    // Pending work cannot justify a successful notification either.
    sqlx::query(
        "UPDATE operations SET status='Queued',task_result_json=NULL WHERE operation_id=$1",
    )
    .bind(id.to_string())
    .execute(&app.db)
    .await?;
    assert!(
        notifications::page(&app, alice.view.account_id, 0)
            .await
            .is_err()
    );
    let mut mismatched = result.clone();
    mismatched.task_storage_id = EntityId::from_bytes([92; 16]);
    sqlx::query(
        "UPDATE operations SET status='Finalized',task_result_json=$1 WHERE operation_id=$2",
    )
    .bind(serde_json::to_string(&mismatched)?)
    .bind(id.to_string())
    .execute(&app.db)
    .await?;
    assert!(operations::read(&app, &alice, id).await.is_err());
    sqlx::query("UPDATE operations SET task_result_json=$1,command_json=$2 WHERE operation_id=$3")
        .bind(serde_json::to_string(&result)?)
        .bind(serde_json::to_string(&create())?)
        .bind(id.to_string())
        .execute(&app.db)
        .await?;
    assert!(operations::read(&app, &alice, id).await.is_err());
    assert!(
        notifications::page(&app, alice.view.account_id, 0)
            .await
            .is_err()
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_history_constraints_preserve_truthful_source_and_existing_rows() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "task_constraint_alice").await?;
    let id = OperationId::from_bytes([65; 16]);
    let (_, result, event) = retained_operation(&app, &alice, id, "Finalized").await?;
    let notification = retained_notification(&app, &event).await?;
    for update in [
        "UPDATE operations SET receipt_json='{}' WHERE task_operation",
        "UPDATE operations SET signed_json='{}' WHERE task_operation",
        "UPDATE operations SET signable_payload=decode('01','hex') WHERE task_operation",
        "UPDATE operations SET possibly_submitted=TRUE WHERE task_operation",
        "UPDATE operations SET catalog_operation=TRUE WHERE task_operation",
        "UPDATE operations SET provider_instance_id=NULL WHERE task_operation",
        "UPDATE operations SET task_result_json=NULL WHERE task_operation",
        "UPDATE operations SET status='Submitted' WHERE task_operation",
        "UPDATE operations SET status='Queued' WHERE task_operation",
        "UPDATE operations SET task_operation=FALSE WHERE task_operation",
        "UPDATE notifications SET provider_cursor=1 WHERE api_operation_id IS NOT NULL",
        "UPDATE notifications SET api_operation_id=NULL WHERE api_operation_id IS NOT NULL",
    ] {
        assert!(sqlx::query(update).execute(&app.db).await.is_err());
    }
    assert!(retained_notification(&app, &event).await.is_err());
    let provider = operations::enqueue(&app, &alice, create(), None).await?;
    assert!(
        sqlx::query("UPDATE operations SET status='Queued' WHERE operation_id=$1")
            .bind(provider.operation_id.to_string())
            .execute(&app.db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE operations SET task_result_json=$1 WHERE operation_id=$2")
            .bind(serde_json::to_string(&result)?)
            .bind(provider.operation_id.to_string())
            .execute(&app.db)
            .await
            .is_err()
    );
    // Provider finality keeps its old semantics: it need not have an API result.
    sqlx::query("UPDATE operations SET status='Finalized' WHERE operation_id=$1")
        .bind(provider.operation_id.to_string())
        .execute(&app.db)
        .await?;
    assert!(
        operations::read(&app, &alice, provider.operation_id)
            .await?
            .task_result
            .is_none()
    );
    // An API notification cannot bind to a provider operation, despite the FK.
    sqlx::query("UPDATE notifications SET api_operation_id=$1 WHERE notification_id=$2")
        .bind(provider.operation_id.to_string())
        .bind(notification)
        .execute(&app.db)
        .await?;
    assert!(
        notifications::page(&app, alice.view.account_id, 0)
            .await
            .is_err()
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_history_migration_preserves_actual_pre_bridge_rows() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "task_upgrade_alice").await?;
    let operation = operations::enqueue(&app, &alice, create(), None).await?;
    sqlx::query("UPDATE operations SET status='Finalized' WHERE operation_id=$1")
        .bind(operation.operation_id.to_string())
        .execute(&app.db)
        .await?;
    notifications::persist(
        &app,
        0,
        ProviderEvents {
            provider_instance_id: info().provider_instance_id,
            events: vec![DomainEvent {
                provider_instance_id: info().provider_instance_id,
                cursor: 1,
                operation_id: OperationId::from_bytes([93; 16]),
                kind: DomainEventKind::WorkerUpdated,
                project_id: None,
                entity_id: None,
                recipients: vec![alice.view.account_id],
                occurred_at: UnixSeconds::new(1),
                origin: None,
            }],
            next_cursor: 1,
        },
    )
    .await?;
    let expected_operation = operations::read(&app, &alice, operation.operation_id).await?;
    let expected_notifications = notifications::page(&app, alice.view.account_id, 0).await?;
    // Recreate only the predecessor schema in this isolated test database. The
    // retained provider rows, IDs, payloads and sessions are left untouched.
    sqlx::raw_sql("ALTER TABLE notifications DROP CONSTRAINT notifications_event_source;
        ALTER TABLE notifications DROP COLUMN api_operation_id;
        ALTER TABLE notifications ALTER COLUMN provider_cursor SET NOT NULL;
        ALTER TABLE operations DROP CONSTRAINT operations_task_source;
        ALTER TABLE operations DROP COLUMN task_operation;
        ALTER TABLE operations DROP COLUMN task_result_json;
        ALTER TABLE operations ADD CONSTRAINT operations_status_check CHECK(status IN ('AwaitingSignature','ReadyToSubmit','Submitted','OutcomeUnknown','Finalized','Rejected','Expired'));")
        .execute(&app.db).await?;
    let database_url = app.config.database_url.clone();
    drop(app);
    for _ in 0..2 {
        let reopened = App::new(test_config(database_url.clone(), &internal)?).await?;
        assert_eq!(
            operations::read(&reopened, &alice, operation.operation_id).await?,
            expected_operation
        );
        assert_eq!(
            notifications::page(&reopened, alice.view.account_id, 0).await?,
            expected_notifications
        );
        assert!(
            !sqlx::query_scalar::<_, bool>(
                "SELECT task_operation FROM operations WHERE operation_id=$1"
            )
            .bind(operation.operation_id.to_string())
            .fetch_one(&reopened.db)
            .await?
        );
    }
    internal.finish().await?;
    Ok(())
}
