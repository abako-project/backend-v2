use super::*;
use generated_contracts::provider_core::*;
#[tokio::test]
async fn lifecycle_reasons_stage_compact_and_enqueue_atomically() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, _) = register(&app, "reason_owner").await?;
    let mut project = content::source_project()?;
    project.client = owner.view.account_id;
    fake.lock().await.snapshot.projects =
        vec![stage_project(&app, info().provider_instance_id, project.clone()).await?];
    let command = ProviderCommand::CancelProject {
        project_id: project.project_id,
        request: ReasonRequest {
            reason: format!("理由\0{}", "x".repeat(200_000)),
        },
    };
    sqlx::raw_sql("ALTER TABLE lifecycle_reason_documents ADD CONSTRAINT blocked CHECK(false)")
        .execute(&app.db)
        .await?;
    assert!(
        operations::enqueue(&app, &owner, command.clone(), None)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::raw_sql("ALTER TABLE lifecycle_reason_documents DROP CONSTRAINT blocked;ALTER TABLE operations ADD CONSTRAINT blocked CHECK(false)").execute(&app.db).await?;
    assert!(
        operations::enqueue(&app, &owner, command.clone(), None)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM lifecycle_reason_documents")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::raw_sql("ALTER TABLE operations DROP CONSTRAINT blocked")
        .execute(&app.db)
        .await?;
    let operation = operations::enqueue(&app, &owner, command.clone(), None).await?;
    let encoded: String =
        sqlx::query_scalar("SELECT command_json FROM operations WHERE operation_id=$1")
            .bind(operation.operation_id.to_string())
            .fetch_one(&app.db)
            .await?;
    let ProviderCommand::CancelProjectCore { content, .. } = serde_json::from_str(&encoded)? else {
        return Err("compact command".into());
    };
    assert!(!encoded.contains("理由"));
    assert!(encoded.len() < 500);
    let core = ReasonCore {
        project_id: project.project_id,
        milestone_id: None,
        origin: owner.view.account_id,
        content: ReasonReferenceV1::Write(content),
        occurred_at: UnixSeconds::new(1_700_000_000),
        kind: DomainEventKind::ProjectCancelled,
    };
    let payload: String = sqlx::query_scalar("SELECT payload FROM lifecycle_reason_documents")
        .fetch_one(&app.db)
        .await?;
    let record: ReasonMetadataRecordV1 = serde_json::from_str(&payload)?;
    assert_eq!(
        record,
        ReasonMetadataRecordV1::from_command(
            info().provider_instance_id,
            owner.view.account_id,
            operation.operation_id,
            &command
        )
        .ok_or("record")?
    );
    assert!(record.matches_context(info().provider_instance_id, 0, &core));
    assert_eq!(hash(&record.hash_input()), core.content.hash().into_bytes());
    internal.finish().await?;
    Ok(())
}
