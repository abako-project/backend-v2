use super::*;
// Current compact source and explicit V4 target, backed by real PostgreSQL.
async fn cohort() -> Result<(Arc<App>, Arc<Mutex<Source>>, Server), Box<dyn std::error::Error>> {
    let mut fixture = Source::fixture()?;
    // Extend the retained fixture with real content to exercise frozen-text checks.
    let mut task = fixture
        .tasks
        .storages
        .iter()
        .flat_map(|r| &r.storage.tasks)
        .next()
        .ok_or("task fixture")?
        .clone();
    task.task_id = fixture.tasks.next_task_id;
    fixture.tasks.next_task_id = task.task_id.checked_add(1).ok_or("task counter")?;
    let case = fixture.cases.values_mut().next().ok_or("case fixture")?;
    case.milestone.task_storage.tasks.push(task.clone());
    let milestone_id = case.milestone.milestone_id;
    fixture
        .snapshot
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.proposals)
        .flat_map(|p| &mut p.milestones)
        .find(|m| m.milestone_id == milestone_id)
        .ok_or("milestone fixture")?
        .task_storage
        .tasks
        .push(task.clone());
    fixture
        .tasks
        .storages
        .iter_mut()
        .find(|r| r.storage.milestone_id == milestone_id)
        .ok_or("storage fixture")?
        .storage
        .tasks
        .push(task);
    let state = Arc::new(Mutex::new(fixture));
    let server = source(state.clone()).await?;
    let app = app(&server).await?;
    crate::task_state::prepare_cutover(&app).await?;
    state.lock().await.compact = true;
    crate::task_state::activate(&app).await?;
    Ok((app, state, server))
}
fn output() -> Result<std::path::PathBuf, Error> {
    Ok(std::env::temp_dir().join(format!(
        "task-rollback-{}.json",
        OperationId::from_bytes(crate::state::random()?)
    )))
}
async fn retained(app: &App) -> Result<Vec<String>, Error> {
    let mut rows = Vec::new();
    for query in [
        "SELECT row_to_json(t)::text FROM operations t ORDER BY operation_id",
        "SELECT row_to_json(t)::text FROM task_items t ORDER BY task_id",
        "SELECT row_to_json(t)::text FROM task_storages t ORDER BY task_storage_id",
        "SELECT row_to_json(t)::text FROM task_migration_cases t ORDER BY case_id",
        "SELECT row_to_json(t)::text FROM task_freezes t ORDER BY operation_id",
    ] {
        rows.extend(
            sqlx::query_scalar::<_, String>(query)
                .fetch_all(&app.db)
                .await?,
        );
    }
    Ok(rows)
}
#[tokio::test]
async fn task_rollback_rejects_source_drift_target_counter_and_unbound_work() -> TestResult {
    let (app, state, server) = cohort().await?;
    let generation = state.lock().await.tasks.generation;
    let path = output()?;
    crate::task_rollback::export(&app, generation, path.clone()).await?;
    assert!(
        crate::task_rollback::export(&app, generation, path.clone())
            .await
            .is_err()
    );
    state.lock().await.compact = false;
    state.lock().await.tasks.next_task_id += 1;
    assert!(
        crate::task_rollback::complete(&app, path.clone())
            .await
            .is_err()
    );
    state.lock().await.tasks.next_task_id -= 1;
    let payload: String =
        sqlx::query_scalar("SELECT payload FROM task_items ORDER BY task_id LIMIT 1")
            .fetch_one(&app.db)
            .await?;
    let mut task: TaskView = serde_json::from_str(&payload)?;
    task.task.title = "Changed after export\0".into();
    sqlx::query("UPDATE task_items SET payload=$1 WHERE task_id=$2")
        .bind(serde_json::to_string(&task)?)
        .bind(i64::from(task.task_id))
        .execute(&app.db)
        .await?;
    assert!(
        crate::task_rollback::complete(&app, path.clone())
            .await
            .is_err()
    );
    sqlx::query("UPDATE task_items SET payload=$1 WHERE task_id=$2")
        .bind(payload)
        .bind(i64::from(task.task_id))
        .execute(&app.db)
        .await?;
    // A malformed or unbound uncertain row is a barrier before command decoding.
    sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,status,next_attempt_at,expires_at,created_at,possibly_submitted) SELECT $1,principal_id,$2,$3,'{}',$4,'OutcomeUnknown',0,1,0,TRUE FROM principals WHERE is_admin LIMIT 1")
        .bind(OperationId::from_bytes([231;16]).to_string()).bind(WalletId::from_bytes([231;16]).to_string()).bind(AccountId32::from_bytes([231;32]).to_string()).bind(Vec::<u8>::new()).execute(&app.db).await?;
    assert!(matches!(
        crate::task_rollback::complete(&app, path.clone()).await,
        Err(Error::Conflict("task_rollback_operations_pending"))
    ));
    assert!(crate::task_views::owned(&app, generation).await?);
    std::fs::remove_file(path)?;
    server.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_rollback_refuses_lost_rows_and_false_maintained_counts() -> TestResult {
    for query in [
        "DELETE FROM task_items",
        "UPDATE task_generations SET storage_count=storage_count+1",
        "DELETE FROM task_storages",
    ] {
        let (app, state, server) = cohort().await?;
        let generation = state.lock().await.tasks.generation;
        sqlx::query(query).execute(&app.db).await?;
        let path = output()?;
        assert!(
            crate::task_rollback::export(&app, generation, path.clone())
                .await
                .is_err()
        );
        assert!(!path.exists());
        assert!(crate::task_views::owned(&app, generation).await?);
        server.finish().await?;
    }
    Ok(())
}
#[tokio::test]
async fn task_rollback_requires_full_frozen_content_and_atomic_ownership_commit() -> TestResult {
    let (app, state, server) = cohort().await?;
    let generation = state.lock().await.tasks.generation;
    let path = output()?;
    crate::task_rollback::export(&app, generation, path.clone()).await?;
    let before = retained(&app).await?;
    state.lock().await.compact = false;
    let (id, original) = {
        let mut source = state.lock().await;
        let (&id, case) = source.cases.iter_mut().next().ok_or("case")?;
        let original = case.milestone.clone();
        case.milestone.task_storage.tasks[0].task.title = "Tampered frozen content".into();
        let changed = case.milestone.clone();
        let milestone = source
            .snapshot
            .projects
            .iter_mut()
            .flat_map(|p| &mut p.proposals)
            .flat_map(|p| &mut p.milestones)
            .find(|m| m.milestone_id == original.milestone_id)
            .ok_or("milestone")?;
        *milestone = changed;
        (id, original)
    };
    assert!(matches!(
        crate::task_rollback::complete(&app, path.clone()).await,
        Err(Error::Conflict("task_rollback_frozen_mismatch"))
    ));
    assert!(crate::task_views::owned(&app, generation).await?);
    {
        let mut source = state.lock().await;
        source.cases.get_mut(&id).ok_or("case")?.milestone = original.clone();
        let milestone_id = original.milestone_id;
        *source
            .snapshot
            .projects
            .iter_mut()
            .flat_map(|p| &mut p.proposals)
            .flat_map(|p| &mut p.milestones)
            .find(|m| m.milestone_id == milestone_id)
            .ok_or("milestone")? = original;
    }
    sqlx::raw_sql("CREATE FUNCTION reject_task_handoff() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected handoff failure'; END; $$; CREATE TRIGGER reject_task_handoff BEFORE UPDATE ON task_generations FOR EACH ROW WHEN (OLD.api_owned AND NOT NEW.api_owned) EXECUTE FUNCTION reject_task_handoff();").execute(&app.db).await?;
    assert!(
        crate::task_rollback::complete(&app, path.clone())
            .await
            .is_err()
    );
    assert!(crate::task_views::owned(&app, generation).await?);
    assert_eq!(retained(&app).await?, before);
    sqlx::query("DROP TRIGGER reject_task_handoff ON task_generations")
        .execute(&app.db)
        .await?;
    crate::task_rollback::complete(&app, path.clone()).await?;
    crate::task_rollback::complete(&app, path.clone()).await?;
    assert_eq!(retained(&app).await?, before);
    assert!(!crate::task_views::owned(&app, generation).await?);
    std::fs::remove_file(path)?;
    server.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_rollback_keeps_obsolete_api_context_outside_the_current_restore_projection()
-> TestResult {
    let (app, state, server) = cohort().await?;
    let generation = state.lock().await.tasks.generation;
    sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$3,$4,'1',$5)")
        .bind(generation.to_string()).bind(EntityId::from_bytes([211;16]).to_string()).bind(EntityId::from_bytes([212;16]).to_string()).bind(EntityId::from_bytes([213;16]).to_string()).bind(EntityId::from_bytes([214;16]).to_string()).execute(&app.db).await?;
    sqlx::query("UPDATE task_generations SET storage_count=storage_count+1")
        .execute(&app.db)
        .await?;
    let before = retained(&app).await?;
    let path = output()?;
    crate::task_rollback::export(&app, generation, path.clone()).await?;
    let saved: TaskStateV1 = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert!(
        !saved
            .storages
            .iter()
            .any(|r| r.storage.task_storage_id == EntityId::from_bytes([211; 16]))
    );
    state.lock().await.compact = false;
    crate::task_rollback::complete(&app, path.clone()).await?;
    assert_eq!(retained(&app).await?, before);
    std::fs::remove_file(path)?;
    server.finish().await?;
    Ok(())
}
