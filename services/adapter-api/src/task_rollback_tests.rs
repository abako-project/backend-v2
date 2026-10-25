//! Owner-only task handoff checks in actual `PostgreSQL`. No activation API.
use super::*;
use std::path::PathBuf;
fn path() -> Result<PathBuf, Error> {
    Ok(std::env::temp_dir().join(format!(
        "task-rollback-{}.json",
        PrincipalId::from_bytes(crate::state::random()?)
    )))
}
async fn export(c: &Cohort) -> Result<(PathBuf, TaskStateV1), Box<dyn std::error::Error>> {
    let file = path()?;
    crate::task_rollback::export(&c.app, info().provider_instance_id, file.clone()).await?;
    let state = serde_json::from_slice(&std::fs::read(&file)?)?;
    Ok((file, state))
}
async fn owned(c: &Cohort) -> Result<bool, Error> {
    crate::task_views::owned(&c.app, info().provider_instance_id).await
}
async fn retained(c: &Cohort) -> Result<Vec<String>, Error> {
    // Full owned history, not just public responses. The ownership flag is the
    // only intended database change at completion; provenance/counts stay too.
    let mut values = Vec::new();
    for query in [
        "SELECT row_to_json(t)::text FROM operations t ORDER BY operation_id",
        "SELECT row_to_json(t)::text FROM notifications t ORDER BY notification_id",
        "SELECT row_to_json(t)::text FROM task_items t ORDER BY task_id",
        "SELECT row_to_json(t)::text FROM task_storages t ORDER BY task_storage_id",
        "SELECT row_to_json(t)::text FROM task_freezes t ORDER BY operation_id",
    ] {
        values.extend(
            sqlx::query_scalar::<_, String>(query)
                .fetch_all(&c.app.db)
                .await?,
        );
    }
    Ok(values)
}
#[tokio::test]
async fn task_rollback_exports_current_context_and_keeps_history_on_verified_retry() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let mut command = c.create();
    if let ProviderCommand::CreateTask { task, .. } = &mut command {
        task.title = "second, then deleted".into();
    }
    c.run(&c.owner, command).await?;
    c.run(
        &c.owner,
        ProviderCommand::DeleteTask {
            project_id: c.project.project_id,
            task_storage_id: c.storage(),
            task_id: 10,
            expected_revision: c.project.proposals[0].revision,
        },
    )
    .await?;
    // Proposal revision is a live core fact, not the backfill row's old revision.
    c.fake.lock().await.snapshot.projects[0].proposals[0].revision += 1;
    let before = retained(&c).await?;
    let (file, state) = export(&c).await?;
    assert_eq!(state.next_task_id, 11);
    assert_eq!(
        state.storages[0].proposal_revision,
        c.project.proposals[0].revision + 1
    );
    assert_eq!(state.storages[0].storage.tasks[0].task, c.definition);
    assert!(owned(&c).await?);
    assert!(
        crate::task_rollback::export(&c.app, state.generation, file.clone())
            .await
            .is_err()
    );
    // Readback from the restored provider must include current data and allocator.
    *c.task_source.lock().await = Some(state.clone());
    c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0].task_storage =
        state.storages[0].storage.clone();
    crate::task_rollback::complete(&c.app, file.clone()).await?;
    assert!(!owned(&c).await?);
    crate::task_rollback::complete(&c.app, file.clone()).await?;
    assert_eq!(retained(&c).await?, before);
    std::fs::remove_file(file)?;
    c.internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_rollback_refuses_source_drift_target_mismatch_and_unbound_work() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let (file, state) = export(&c).await?;
    // The raw source still has stale/empty tasks and must not establish restore.
    assert!(
        crate::task_rollback::complete(&c.app, file.clone())
            .await
            .is_err()
    );
    assert!(owned(&c).await?);
    *c.task_source.lock().await = Some(state);
    c.run(&c.owner, c.create()).await?;
    assert!(
        crate::task_rollback::complete(&c.app, file.clone())
            .await
            .is_err()
    );
    assert!(owned(&c).await?);
    let pending = operations::enqueue(&c.app, &c.owner, c.create(), None).await?;
    assert!(
        crate::task_rollback::export(&c.app, info().provider_instance_id, path()?)
            .await
            .is_err()
    );
    sqlx::query("UPDATE operations SET status='Rejected',possibly_submitted=TRUE,error_code='uncertain' WHERE operation_id=$1").bind(pending.operation_id.to_string()).execute(&c.app.db).await?;
    assert!(
        crate::task_rollback::export(&c.app, info().provider_instance_id, path()?)
            .await
            .is_err()
    );
    sqlx::query("UPDATE operations SET error_code='provider_rejected' WHERE operation_id=$1")
        .bind(pending.operation_id.to_string())
        .execute(&c.app.db)
        .await?;
    let (released, _) = export(&c).await?;
    std::fs::remove_file(released)?;
    let freeze = operations::enqueue(
        &c.app,
        &c.client,
        ProviderCommand::CancelProject {
            project_id: c.project.project_id,
            request: ReasonRequest {
                reason: "Rollback hold".into(),
            },
        },
        None,
    )
    .await?;
    sqlx::query("UPDATE operations SET status='Rejected' WHERE operation_id=$1")
        .bind(freeze.operation_id.to_string())
        .execute(&c.app.db)
        .await?;
    // Terminal transport state alone cannot release a retained pending hold.
    assert!(
        crate::task_rollback::export(&c.app, info().provider_instance_id, path()?)
            .await
            .is_err()
    );

    std::fs::remove_file(file)?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_rollback_requires_frozen_task_equality_and_atomic_ownership_commit() -> TestResult {
    let (c, slot, _, _) = super::views::case_fixture().await?;
    let accepted = c.tasks().await?;
    let mut changed = accepted[0].clone();
    changed.task.title = "Corrupt post-freeze current value".into();
    sqlx::query("UPDATE task_items SET payload=$1")
        .bind(serde_json::to_string(&changed)?)
        .execute(&c.app.db)
        .await?;
    c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
        .task_storage
        .tasks = vec![changed.clone()];
    slot.lock()
        .await
        .as_mut()
        .ok_or("case")?
        .milestone
        .task_storage
        .tasks = vec![changed];
    let (file, state) = export(&c).await?;
    *c.task_source.lock().await = Some(state);
    assert!(matches!(
        crate::task_rollback::complete(&c.app, file.clone()).await,
        Err(Error::Conflict("task_rollback_frozen_mismatch"))
    ));
    assert!(owned(&c).await?);
    std::fs::remove_file(file)?;
    sqlx::query("UPDATE task_items SET payload=$1")
        .bind(serde_json::to_string(&accepted[0])?)
        .execute(&c.app.db)
        .await?;
    c.fake.lock().await.snapshot.projects[0].proposals[0].milestones[0]
        .task_storage
        .tasks = accepted.clone();
    slot.lock()
        .await
        .as_mut()
        .ok_or("case")?
        .milestone
        .task_storage
        .tasks = accepted;
    let (file, state) = export(&c).await?;
    *c.task_source.lock().await = Some(state);
    let before = retained(&c).await?;
    sqlx::query("CREATE FUNCTION fail_task_handoff() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'rollback fixture failure'; END $$").execute(&c.app.db).await?;
    sqlx::query("CREATE TRIGGER fail_task_handoff BEFORE UPDATE ON task_generations FOR EACH ROW EXECUTE FUNCTION fail_task_handoff()").execute(&c.app.db).await?;
    assert!(
        crate::task_rollback::complete(&c.app, file.clone())
            .await
            .is_err()
    );
    assert!(owned(&c).await?);
    assert_eq!(retained(&c).await?, before);
    sqlx::query("DROP TRIGGER fail_task_handoff ON task_generations")
        .execute(&c.app.db)
        .await?;
    crate::task_rollback::complete(&c.app, file.clone()).await?;
    crate::task_rollback::complete(&c.app, file.clone()).await?;
    assert!(!owned(&c).await?);
    assert_eq!(retained(&c).await?, before);
    std::fs::remove_file(file)?;
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_rollback_rejects_lost_rows_and_bad_counts_but_keeps_retained_orphans() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let (file, state) = export(&c).await?;
    *c.task_source.lock().await = Some(state);
    sqlx::query("DELETE FROM task_items")
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        crate::task_rollback::export(&c.app, info().provider_instance_id, path()?).await,
        Err(Error::Dependency)
    ));
    assert!(matches!(
        crate::task_rollback::complete(&c.app, file.clone()).await,
        Err(Error::Dependency)
    ));
    assert!(owned(&c).await?);
    std::fs::remove_file(file)?;
    // Return to the known empty graph, then model retained obsolete context.
    // Its real row count matters; stale revision/narrative do not enter export.
    sqlx::query("UPDATE task_generations SET task_count=0,storage_count=99")
        .execute(&c.app.db)
        .await?;
    assert!(
        crate::task_rollback::export(&c.app, info().provider_instance_id, path()?)
            .await
            .is_err()
    );
    sqlx::query("UPDATE task_generations SET storage_count=1")
        .execute(&c.app.db)
        .await?;
    let orphan = EntityId::from_bytes([99; 16]);
    sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$2,$2,'18446744073709551615',$2)")
        .bind(info().provider_instance_id.to_string()).bind(orphan.to_string()).execute(&c.app.db).await?;
    let orphan_task = TaskView {
        task_id: 9,
        reporter: c.owner.view.account_id,
        created_at: UnixSeconds::new(1),
        updated_at: UnixSeconds::new(1),
        task: c.definition.clone(),
    };
    sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,9,$2,0,$3,$4)")
        .bind(info().provider_instance_id.to_string()).bind(orphan.to_string()).bind(serde_json::to_string(&orphan_task)?).bind(vec![c.worker.view.account_id.to_string()]).execute(&c.app.db).await?;
    sqlx::query("UPDATE task_generations SET storage_count=2,task_count=1")
        .execute(&c.app.db)
        .await?;
    let before = retained(&c).await?;
    let (file, state) = export(&c).await?;
    assert_eq!(state.storages.len(), 1);
    assert!(state.storages[0].storage.tasks.is_empty());
    assert_eq!(state.next_task_id, 10);
    *c.task_source.lock().await = Some(state);
    crate::task_rollback::complete(&c.app, file.clone()).await?;
    assert_eq!(retained(&c).await?, before);
    std::fs::remove_file(file)?;
    c.internal.finish().await?;
    Ok(())
}
