use super::*;
use generated_contracts::provider_core::*;
#[tokio::test]
async fn lifecycle_reasons_stage_compact_and_enqueue_atomically() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, _) = register(&app, "reason_owner").await?;
    let mut project = content_copies::source_project()?;
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
    assert_eq!(
        crate::reason_metadata::record(&app, info().provider_instance_id, 0, core).await?,
        ReasonMetadataRecordV1::from_command(
            info().provider_instance_id,
            owner.view.account_id,
            operation.operation_id,
            &command
        )
        .ok_or("record")?
    );
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn reason_exchange_pages_verifies_order_and_rolls_back_duplicate_files() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let source = app(&internal).await?;
    let target = app(&internal).await?;
    let generation = info().provider_instance_id;
    let mut reasons = Vec::new();
    let mut tx = source.db.begin().await?;
    for index in 0u32..300 {
        let record = ReasonMetadataRecordV1 {
            generation,
            provenance: ReasonProvenanceV1::Migration {
                source_archive_hash: PayloadHash::from_bytes([80; 32]),
                record_index: u64::from(index),
                origin: info().root_account,
                occurred_at: UnixSeconds::new(1_700_000_000 + u64::from(index)),
            },
            project_id: EntityId::from_bytes([2; 16]),
            milestone_id: None,
            kind: DomainEventKind::PlanningDisputed,
            reason: format!("Retained\0{index}"),
        };
        let fingerprint = hash(&record.hash_input());
        crate::reason_metadata::insert(
            &mut tx,
            generation,
            fingerprint,
            &serde_json::to_string(&record)?,
        )
        .await?;
        reasons.push(ReasonCore {
            project_id: record.project_id,
            milestone_id: None,
            origin: info().root_account,
            content: ReasonReferenceV1::Migration(PayloadHash::from_bytes(fingerprint)),
            occurred_at: UnixSeconds::new(1_700_000_000 + u64::from(index)),
            kind: record.kind,
        });
    }
    tx.commit().await?;
    fake.lock().await.reasons = reasons;
    let path = std::env::temp_dir().join(format!(
        "reason-metadata-{}.ndjson",
        OperationId::from_bytes(crate::state::random()?)
    ));
    assert_eq!(
        crate::reason_metadata_io::export(&source, path.clone()).await?,
        300
    );
    assert_eq!(
        crate::reason_metadata_io::import(&target, path.clone()).await?,
        300
    );
    assert_eq!(
        crate::reason_metadata_io::import(&target, path.clone()).await?,
        300
    );
    let original = tokio::fs::read_to_string(&path).await?;
    let first = original.lines().nth(1).ok_or("first")?;
    let bad = path.with_extension("duplicate.ndjson");
    tokio::fs::write(&bad, format!("{original}{first}\n")).await?;
    let empty = app(&internal).await?;
    assert!(
        crate::reason_metadata_io::import(&empty, bad.clone())
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM lifecycle_reason_documents")
            .fetch_one(&empty.db)
            .await?,
        0
    );
    // Reordered core positions must not validate migration provenance.
    fake.lock().await.reasons.swap(0, 1);
    assert!(
        crate::reason_metadata_io::import(&target, path.clone())
            .await
            .is_err()
    );
    tokio::fs::remove_file(path).await?;
    tokio::fs::remove_file(bad).await?;
    internal.finish().await?;
    Ok(())
}
