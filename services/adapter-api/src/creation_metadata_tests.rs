use super::*;
use generated_contracts::provider_core::{
    CreationContentV1, CreationMetadataRecordV1, MetadataProvenanceV1,
};
use parity_scale_codec::{DecodeAll, Encode};
use serde_json::{Value, json};

#[test]
fn creation_metadata_sha256_matches_independent_vectors_and_binds_context() -> TestResult {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../specs/0015-adapter-core-boundary/creation-metadata-vectors.json"
    ))?;
    for vector in fixture["vectors"].as_array().ok_or("vectors missing")? {
        let record: CreationMetadataRecordV1 = serde_json::from_value(vector["record"].clone())?;
        let digest = PayloadHash::from_bytes(hash(&record.hash_input()));
        assert_eq!(digest.to_string(), vector["sha256"]);
        let mut changed = record.clone();
        changed.generation = ProviderInstanceId::from_bytes([42; 16]);
        assert_ne!(digest.into_bytes(), hash(&changed.hash_input()));
        changed = record.clone();
        match &mut changed.provenance {
            MetadataProvenanceV1::Creation { author, .. } => {
                *author = AccountId32::from_bytes([42; 32]);
            }
        }
        assert_ne!(digest.into_bytes(), hash(&changed.hash_input()));
        changed = record.clone();
        match &mut changed.content {
            CreationContentV1::WorkerName { display_name } => display_name.push(' '),
            CreationContentV1::ProjectText { description, .. } => description.push(' '),
        }
        assert_ne!(digest.into_bytes(), hash(&changed.hash_input()));
    }
    Ok(())
}

#[tokio::test]
async fn creation_metadata_keeps_exact_large_requests_and_queues_only_compact_facts() -> TestResult
{
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (session, _) = register(&app, "creation_content").await?;
    let worker: RegisterWorkerRequest = serde_json::from_value(json!({
        "displayName":format!("Zoë\0{}", "名".repeat(60_000)),
        "qualifications":{"roleIds":[],"skillIds":[]},
        "calendar":{"defaultWeeklyMinutes":2400,"overrides":[]}
    }))?;
    let commands = [
        ProviderCommand::RegisterWorker(worker),
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Cafe\u{301}\0".into(),
            description: format!("\r\n{}", "x".repeat(200_000)),
        }),
    ];
    for (index, command) in commands.into_iter().enumerate() {
        let operation = OperationId::from_bytes([u8::try_from(index + 10)?; 16]);
        let original_bytes = command.encode();
        let expected = CreationMetadataRecordV1::from_creation_command(
            info().provider_instance_id,
            session.view.account_id,
            operation,
            &command,
        )
        .ok_or("expected creation record")?;
        let queued = operations::enqueue(&app, &session, command.clone(), Some(operation)).await?;
        let repeated = operations::enqueue(&app, &session, command, Some(operation)).await?;
        assert_eq!(queued, repeated);
        let row = sqlx::query("SELECT command_bytes,request_command_bytes,provider_instance_id FROM operations WHERE operation_id=$1")
            .bind(operation.to_string()).fetch_one(&app.db).await?;
        let bytes: Vec<u8> = row.try_get("command_bytes")?;
        assert!(bytes.len() < 128);
        let queued = ProviderCommand::decode_all(&mut &bytes[..])?;
        let commitment = match queued {
            ProviderCommand::RegisterWorkerCore(request) => request.content,
            ProviderCommand::CreateProjectCore(content) => content,
            _ => return Err("creation was not compacted".into()),
        };
        assert_eq!(commitment.operation_id, operation);
        assert_eq!(commitment.hash.into_bytes(), hash(&expected.hash_input()));
        assert_eq!(
            row.try_get::<Vec<u8>, _>("request_command_bytes")?,
            original_bytes
        );
        assert_eq!(
            row.try_get::<String, _>("provider_instance_id")?,
            expected.generation.to_string()
        );
        let select = if index == 0 {
            "SELECT content_hash,payload FROM worker_directory_metadata"
        } else {
            "SELECT content_hash,payload FROM project_creation_metadata"
        };
        let rows = sqlx::query(select).fetch_all(&app.db).await?;
        assert_eq!(rows.len(), 1);
        let stored: CreationMetadataRecordV1 = serde_json::from_str(rows[0].try_get("payload")?)?;
        assert_eq!(stored, expected);
        assert_eq!(
            rows[0].try_get::<Vec<u8>, _>("content_hash")?,
            hash(&stored.hash_input())
        );
    }
    assert!(fake.lock().await.jobs.is_empty());
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn creation_metadata_and_operation_insert_are_atomic_and_retryable() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (session, _) = register(&app, "creation_atomic").await?;
    let operation = OperationId::from_bytes([20; 16]);
    sqlx::raw_sql("CREATE FUNCTION deny_operation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END $$; CREATE TRIGGER deny_operation BEFORE INSERT ON operations FOR EACH ROW EXECUTE FUNCTION deny_operation()")
        .execute(&app.db).await?;
    assert!(
        operations::enqueue(&app, &session, create(), Some(operation))
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_creation_metadata")
            .fetch_one(&app.db)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::raw_sql("DROP TRIGGER deny_operation ON operations; DROP FUNCTION deny_operation()")
        .execute(&app.db)
        .await?;
    operations::enqueue(&app, &session, create(), Some(operation)).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_creation_metadata")
            .fetch_one(&app.db)
            .await?,
        1
    );
    let changed = ProviderCommand::CreateProject(CreateProjectRequest {
        title: "Changed".into(),
        description: "Description".into(),
    });
    assert!(matches!(
        operations::enqueue(&app, &session, changed, Some(operation)).await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    // Roll back the transaction on the other side as well: failed metadata
    // persistence cannot leave a queued command without its preimage.
    sqlx::raw_sql("CREATE FUNCTION reject_metadata() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture failure'; END $$; CREATE TRIGGER deny_metadata BEFORE INSERT ON project_creation_metadata FOR EACH ROW EXECUTE FUNCTION reject_metadata()")
        .execute(&app.db).await?;
    let second = OperationId::from_bytes([21; 16]);
    assert!(
        operations::enqueue(&app, &session, create(), Some(second))
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
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_creation_metadata")
            .fetch_one(&app.db)
            .await?,
        1
    );
    sqlx::raw_sql(
        "DROP TRIGGER deny_metadata ON project_creation_metadata; DROP FUNCTION reject_metadata()",
    )
    .execute(&app.db)
    .await?;
    operations::enqueue(&app, &session, create(), Some(second)).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        2
    );
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn creation_metadata_generation_is_bound_before_signing() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (session, _) = register(&app, "creation_generation").await?;
    let operation = operations::enqueue(&app, &session, create(), None)
        .await?
        .operation_id;
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([22; 16]);
    let job = operations::claim(&app).await?.ok_or("queued creation")?;
    operations::process(&app, job).await?;
    let result = operations::read(&app, &session, operation).await?;
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

#[tokio::test]
async fn creation_metadata_checks_occupied_hash_and_queue_capacity() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (session, _) = register(&app, "creation_admission").await?;
    let operation = OperationId::from_bytes([30; 16]);
    let record = CreationMetadataRecordV1::from_creation_command(
        info().provider_instance_id,
        session.view.account_id,
        operation,
        &create(),
    )
    .ok_or("creation record")?;
    sqlx::query("INSERT INTO project_creation_metadata(provider_instance_id,content_hash,payload) VALUES($1,$2,'tampered')")
        .bind(record.generation.to_string()).bind(hash(&record.hash_input()).as_slice())
        .execute(&app.db).await?;
    assert!(matches!(
        operations::enqueue(&app, &session, create(), Some(operation)).await,
        Err(Error::Dependency)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::query("DELETE FROM project_creation_metadata")
        .execute(&app.db)
        .await?;
    for index in 0..32 {
        operations::enqueue(
            &app,
            &session,
            create(),
            Some(OperationId::from_bytes([index; 16])),
        )
        .await?;
    }
    assert!(matches!(
        operations::enqueue(&app, &session, create(), None).await,
        Err(Error::Capacity)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM project_creation_metadata")
            .fetch_one(&app.db)
            .await?,
        32
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        32
    );
    assert!(fake.lock().await.jobs.is_empty());
    assert_eq!(fake.lock().await.submissions, 0);
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}

#[tokio::test]
async fn creation_metadata_failure_affects_presentation_without_blocking_authorized_core_or_task_reads()
-> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, cookie) = register(&app, "metadata_views").await?;
    let (_, stranger) = register(&app, "metadata_stranger").await?;
    let generation = info().provider_instance_id;
    let mut project = content::source_project()?;
    project.client = owner.view.account_id;
    project.title = "Confirmed\0title".into();
    let op = operations::enqueue(
        &app,
        &owner,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: project.title.clone(),
            description: project.description.clone(),
        }),
        None,
    )
    .await?;
    let queued: String =
        sqlx::query_scalar("SELECT command_json FROM operations WHERE operation_id=$1")
            .bind(op.operation_id.to_string())
            .fetch_one(&app.db)
            .await?;
    let ProviderCommand::CreateProjectCore(commitment) = serde_json::from_str(&queued)? else {
        return Err("compact creation missing".into());
    };
    stage_proposal_documents(&app, generation, &project).await?;
    stage_task_content(&app, generation, project.clone()).await?;
    let mut core = core_project(&project, generation)?.0;
    core.content = generated_contracts::provider_core::MetadataReferenceV1::Creation(commitment);
    let server = Server::start(crate::http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/api/projects/{}", server.url, project.project_id);
    // Pending content alone is never a public entity.
    assert_eq!(
        client
            .get(&url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    fake.lock().await.snapshot.projects.push(core);
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
    sqlx::query("UPDATE project_creation_metadata SET payload='tampered'")
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
    let storage_url = format!(
        "{}/api/task-storages/{}",
        server.url, storage.task_storage_id
    );
    assert_eq!(
        client
            .get(storage_url)
            .header(header::COOKIE, &cookie)
            .send()
            .await?
            .json::<TaskStorageView>()
            .await?,
        *storage
    );
    let cancellation = operations::enqueue(
        &app,
        &owner,
        ProviderCommand::CancelProject {
            project_id: project.project_id,
            request: ReasonRequest {
                reason: "Core authorization still available".into(),
            },
        },
        None,
    )
    .await?;
    assert_eq!(
        cancellation.status,
        ProviderOperationStatus::AwaitingSignature
    );
    server.finish().await?;
    internal.finish().await?;
    app.db.close().await;
    Ok(())
}
