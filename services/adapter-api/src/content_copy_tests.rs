use super::*;
use serde_json::{Value, json};

fn source_project() -> Result<ProjectView, Box<dyn std::error::Error>> {
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
fn source_worker() -> Result<WorkerView, Box<dyn std::error::Error>> {
    let trace: Value = serde_json::from_str(include_str!(
        "../../mock-provider/src/domain/fixtures/financial-signed.json"
    ))?;
    Ok(serde_json::from_value(trace["final"][0][1][0].clone())?)
}

#[tokio::test]
async fn provider_content_copies_preserve_large_values_scope_and_concurrent_versions() -> TestResult
{
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let mut original = source_project()?;
    original.description = "p".repeat(200_000);
    original.proposals[0].description = "q".repeat(80_000);
    let mut newer = original.clone();
    newer.title = "Changed title".into();
    let generation = info().provider_instance_id;
    let (old, new) = tokio::try_join!(
        crate::project_content::compose(&app, generation, original.clone()),
        crate::project_content::compose(&app, generation, newer.clone()),
    )?;
    assert_eq!(old, original);
    assert_eq!(new, newer);
    assert_eq!(
        crate::project_content::compose(&app, generation, original.clone()).await?,
        original
    );
    let second_generation = ProviderInstanceId::from_bytes([2; 16]);
    assert_eq!(
        crate::project_content::compose(&app, second_generation, original.clone()).await?,
        original
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 3);
    let payloads: Vec<String> = sqlx::query_scalar("SELECT payload FROM project_content_copies")
        .fetch_all(&app.db)
        .await?;
    for payload in payloads {
        let payload: Value = serde_json::from_str(&payload)?;
        assert!(payload.get("client").is_none());
        assert!(payload.get("executionEscrow").is_none());
        assert!(
            payload["proposals"][0]["milestones"][0]
                .get("assignments")
                .is_none()
        );
    }
    let mut worker = source_worker()?;
    worker.display_name = format!("\0{}", "n".repeat(MAX_SIGNABLE_BYTES - 201));
    let expected = worker.clone();
    crate::profiles::directory::compose(&app, generation, &mut worker).await?;
    assert_eq!(worker, expected);
    crate::profiles::directory::compose(&app, generation, &mut worker).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM worker_directory_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 1);
    sqlx::query("UPDATE project_content_copies SET payload=jsonb_set(payload::jsonb,'{title}','\"tampered\"')::text WHERE provider_instance_id=$1")
        .bind(generation.to_string()).execute(&app.db).await?;
    assert!(matches!(
        crate::project_content::compose(&app, generation, original.clone()).await,
        Err(Error::Dependency)
    ));
    // Other generations and the provider source remain independent.
    assert_eq!(
        crate::project_content::compose(&app, second_generation, original.clone()).await?,
        original
    );
    sqlx::query(r#"UPDATE worker_directory_copies SET display_name_json='"tampered"'"#)
        .execute(&app.db)
        .await?;
    assert!(matches!(
        crate::profiles::directory::compose(&app, generation, &mut worker).await,
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
    fake.lock().await.snapshot.projects.push(project.clone());
    fake.lock().await.snapshot.workers.push(worker.clone());
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
    assert_eq!(count, 1);
    assert_eq!(fake.lock().await.snapshot.projects, vec![project]);
    assert_eq!(fake.lock().await.submissions, 0);
    assert!(fake.lock().await.jobs.is_empty());
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn provider_content_copies_failed_insert_does_not_change_source_and_retry_backfills()
-> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let project = source_project()?;
    fake.lock().await.snapshot.projects.push(project.clone());
    sqlx::query("ALTER TABLE project_content_copies ADD CONSTRAINT reject_copy CHECK (false)")
        .execute(&app.db)
        .await?;
    assert!(
        crate::project_content::compose(&app, info().provider_instance_id, project.clone())
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM project_content_copies")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(count, 0);
    assert_eq!(fake.lock().await.snapshot.projects, vec![project.clone()]);
    sqlx::query("ALTER TABLE project_content_copies DROP CONSTRAINT reject_copy")
        .execute(&app.db)
        .await?;
    assert_eq!(
        crate::project_content::compose(&app, info().provider_instance_id, project.clone()).await?,
        project
    );
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
