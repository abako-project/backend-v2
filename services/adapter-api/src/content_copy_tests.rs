use super::*;
use serde_json::{Value, json};

pub(super) fn source_project() -> Result<ProjectView, Box<dyn std::error::Error>> {
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.proposals[0].milestones[0]
        .task_storage
        .tasks
        .push(TaskView {
            task_id: 1,
            reporter: project.coordinator,
            created_at: UnixSeconds::new(100),
            updated_at: UnixSeconds::new(100),
            task: serde_json::from_value(
                doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone(),
            )?,
        });
    Ok(project)
}
#[derive(serde::Deserialize)]
struct WorkerSource {
    checkpoint: WorkerCheckpoint,
}
#[derive(serde::Deserialize)]
struct WorkerCheckpoint {
    workers: std::collections::BTreeMap<AccountId32, WorkerView>,
}

pub(super) fn source_worker() -> Result<WorkerView, Box<dyn std::error::Error>> {
    let trace: Value = serde_json::from_str(include_str!(
        "../../mock-provider/src/domain/fixtures/predecessor-migration.json"
    ))?;
    let archive: WorkerSource =
        serde_json::from_str(trace["archives"][1]["archive"].as_str().ok_or("archive")?)?;
    archive
        .checkpoint
        .workers
        .into_values()
        .next()
        .ok_or_else(|| "worker".into())
}

#[tokio::test]
async fn project_composition_preserves_metadata_versions_and_current_api_task_content() -> TestResult
{
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let mut original = source_project()?;
    original.description = "p".repeat(200_000);
    original.proposals[0].description = "q".repeat(80_000);
    let mut newer = original.clone();
    newer.proposals[0].title = "Changed title".into();
    newer.proposals[0].milestones[0].task_storage.tasks[0]
        .task
        .title = "Changed tracking task".into();
    let generation = info().provider_instance_id;
    let (old, new) = tokio::try_join!(
        crate::project_content::compose(
            &app,
            generation,
            stage_project(&app, generation, original.clone()).await?
        ),
        crate::project_content::compose(
            &app,
            generation,
            stage_project(&app, generation, newer.clone()).await?
        ),
    )?;
    let mut expected_old = original.clone();
    expected_old.proposals[0].milestones[0].task_storage.tasks[0]
        .task
        .title
        .clone_from(
            &newer.proposals[0].milestones[0].task_storage.tasks[0]
                .task
                .title,
        );
    assert!(old == expected_old);
    assert_eq!(new, newer);
    assert_eq!(
        crate::project_content::compose(
            &app,
            generation,
            stage_project(&app, generation, original.clone()).await?
        )
        .await?,
        original
    );
    let second_generation = ProviderInstanceId::from_bytes([2; 16]);
    assert_eq!(
        crate::project_content::compose(
            &app,
            second_generation,
            stage_project(&app, second_generation, original.clone()).await?
        )
        .await?,
        original
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 0);
    let mut worker = source_worker()?;
    worker.display_name = format!("\0{}", "n".repeat(MAX_SIGNABLE_BYTES - 201));
    let expected = worker.clone();
    let core = stage_worker(&app, generation, worker.clone()).await?;
    assert_eq!(
        crate::profiles::directory::compose(&app, generation, core.clone()).await?,
        expected
    );
    assert_eq!(worker, expected);
    assert_eq!(
        crate::profiles::directory::compose(&app, generation, core.clone()).await?,
        expected
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM worker_directory_metadata")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 1);
    sqlx::query("UPDATE task_items SET payload='tampered' WHERE provider_instance_id=$1")
        .bind(generation.to_string())
        .execute(&app.db)
        .await?;
    assert!(matches!(
        crate::project_content::compose(&app, generation, core_project(&original, generation)?.0)
            .await,
        Err(Error::Dependency)
    ));
    // Other generations and the provider source remain independent.
    assert_eq!(
        crate::project_content::compose(
            &app,
            second_generation,
            stage_project(&app, second_generation, original.clone()).await?
        )
        .await?,
        original
    );
    sqlx::query(r#"UPDATE worker_directory_metadata SET payload='"tampered"'"#)
        .execute(&app.db)
        .await?;
    assert!(matches!(
        crate::profiles::directory::compose(&app, generation, core.clone()).await,
        Err(Error::Dependency)
    ));
    assert_eq!(worker, expected);
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn provider_content_copies_preserve_public_views_visibility_and_read_only_backfill()
-> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (client, cookie) = register(&app, "content_owner").await?;
    let (_, stranger_cookie) = register(&app, "content_stranger").await?;
    let mut project = source_project()?;
    project.client = client.view.account_id;
    let id = project.project_id;
    let storage_id = project.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    let worker = source_worker()?;
    fake.lock()
        .await
        .snapshot
        .projects
        .push(stage_project(&app, info().provider_instance_id, project.clone()).await?);
    fake.lock()
        .await
        .snapshot
        .workers
        .push(stage_worker(&app, info().provider_instance_id, worker.clone()).await?);
    let server = Server::start(crate::http::router(app.clone())).await?;
    let http = reqwest::Client::new();
    let project_url = format!("{}/api/projects/{id}", server.url);
    assert_eq!(
        http.get(&project_url)
            .header(header::COOKIE, &stranger_cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        http.get(format!("{}/api/projects", server.url))
            .header(header::COOKIE, &stranger_cookie)
            .send()
            .await?
            .json::<Value>()
            .await?,
        json!([])
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(
        count, 0,
        "invisible projects must not be copied by a stranger's request"
    );
    assert_eq!(
        http.get(&project_url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .json::<ProjectView>()
            .await?,
        project
    );
    let storage: TaskStorageView = http
        .get(format!("{}/api/task-storages/{storage_id}", server.url))
        .header(header::COOKIE, &cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(storage, project.proposals[0].milestones[0].task_storage);
    let task: TaskView = http
        .get(format!(
            "{}/api/task-storages/{storage_id}/tasks/1",
            server.url
        ))
        .header(header::COOKIE, &cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(task, storage.tasks[0]);
    let workers: Value = http
        .get(format!("{}/api/workers", server.url))
        .header(header::COOKIE, &cookie)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(workers[0]["displayName"], worker.display_name);
    assert_eq!(crate::backfill_content(&app).await?, (1, 1));
    assert_eq!(crate::backfill_content(&app).await?, (1, 1));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 0);
    assert_eq!(
        fake.lock().await.snapshot.projects,
        vec![core_project(&project, info().provider_instance_id)?.0]
    );
    assert_eq!(fake.lock().await.submissions, 0);
    assert!(fake.lock().await.jobs.is_empty());
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn obsolete_project_copy_writes_are_not_required_for_current_views() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let project = source_project()?;
    fake.lock()
        .await
        .snapshot
        .projects
        .push(stage_project(&app, info().provider_instance_id, project.clone()).await?);
    sqlx::query("ALTER TABLE project_content_copies ADD CONSTRAINT reject_copy CHECK (false)")
        .execute(&app.db)
        .await?;
    assert!(
        crate::project_content::compose(
            &app,
            info().provider_instance_id,
            stage_project(&app, info().provider_instance_id, project.clone()).await?
        )
        .await
        .is_ok()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 0);
    assert_eq!(
        fake.lock().await.snapshot.projects,
        vec![core_project(&project.clone(), info().provider_instance_id)?.0]
    );
    sqlx::query("ALTER TABLE project_content_copies DROP CONSTRAINT reject_copy")
        .execute(&app.db)
        .await?;
    assert_eq!(
        crate::project_content::compose(
            &app,
            info().provider_instance_id,
            stage_project(&app, info().provider_instance_id, project.clone()).await?
        )
        .await?,
        project
    );
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "Compare all task write aliases with project routes under one corrupt-copy scenario."
)]
async fn provider_content_copies_cannot_block_raw_task_write_aliases() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (coordinator, cookie) = register(&app, "content_coordinator").await?;
    let mut project = source_project()?;
    project.coordinator = coordinator.view.account_id;
    project.proposals[0].milestones[0].task_storage.tasks[0]
        .task
        .assignees = vec![coordinator.view.account_id];
    let task = project.proposals[0].milestones[0].task_storage.tasks[0]
        .task
        .clone();
    let id = project.project_id;
    let storage = project.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    fake.lock()
        .await
        .snapshot
        .projects
        .push(stage_project(&app, info().provider_instance_id, project.clone()).await?);
    crate::project_content::compose(
        &app,
        info().provider_instance_id,
        stage_project(&app, info().provider_instance_id, project.clone()).await?,
    )
    .await?;
    sqlx::query("UPDATE task_items SET payload='tampered'")
        .execute(&app.db)
        .await?;
    let before: String = sqlx::query_scalar("SELECT payload FROM task_items")
        .fetch_one(&app.db)
        .await?;
    let server = Server::start(crate::http::router(app.clone())).await?;
    let http = reqwest::Client::new();
    let alias = format!("{}/api/task-storages/{storage}/tasks", server.url);
    let nested = format!(
        "{}/api/projects/{id}/task-storages/{storage}/tasks",
        server.url
    );
    for url in [
        format!("{}/api/task-storages/{storage}", server.url),
        format!("{alias}/1"),
    ] {
        assert_eq!(
            http.get(url)
                .header(header::COOKIE, &cookie)
                .send()
                .await?
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    for (index, (method, alias_url, nested_url, body)) in [
        (
            reqwest::Method::POST,
            alias.clone(),
            nested.clone(),
            serde_json::to_value(&task)?,
        ),
        (
            reqwest::Method::PUT,
            format!("{alias}/1"),
            format!("{nested}/1"),
            serde_json::to_value(&task)?,
        ),
        (
            reqwest::Method::PATCH,
            format!("{alias}/1"),
            format!("{nested}/1/progress"),
            json!({"status":"Done","loggedMinutes":2}),
        ),
        (
            reqwest::Method::DELETE,
            format!("{alias}/1"),
            format!("{nested}/1"),
            json!({"expectedRevision":project.proposals[0].revision}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut commands = Vec::new();
        for (offset, url) in [alias_url, nested_url].into_iter().enumerate() {
            let serial = u8::try_from(index * 2 + offset + 1)?;
            let operation = OperationId::from_bytes([serial; 16]);
            let response = http
                .request(method.clone(), url)
                .header(header::COOKIE, &cookie)
                .header(header::ORIGIN, "http://localhost:3000")
                .header("X-CSRF-Token", &coordinator.view.csrf_token)
                .header("Idempotency-Key", operation.to_string())
                .json(&body)
                .send()
                .await?;
            assert_eq!(
                response.status(),
                StatusCode::ACCEPTED,
                "{method} pair {index}"
            );
            let command: String =
                sqlx::query_scalar("SELECT command_json FROM operations WHERE operation_id=$1")
                    .bind(operation.to_string())
                    .fetch_one(&app.db)
                    .await?;
            commands.push(serde_json::from_str::<ProviderCommand>(&command)?);
        }
        assert_eq!(
            commands[0], commands[1],
            "{method} aliases must queue identical native commands"
        );
    }
    let after: String = sqlx::query_scalar("SELECT payload FROM task_items")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(before, after, "writes must not touch the API task payload");
    assert_eq!(
        fake.lock().await.snapshot.projects,
        vec![core_project(&project, info().provider_instance_id)?.0]
    );
    assert_eq!(fake.lock().await.submissions, 0);
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
