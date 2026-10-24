//! Actual `PostgreSQL` task rows/counter preparation; no task authority switches.
use super::*;
use generated_contracts::provider_core::{TaskStateV1, TaskStorageRecordV1};
use parity_scale_codec::Encode;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn inventory() -> Result<TaskStateV1, Box<dyn std::error::Error>> {
    let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut definition: TaskDefinition = serde_json::from_value(
        doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone(),
    )?;
    definition.description = "λ\0".repeat(20_000);
    definition.assignees = vec![
        AccountId32::from_bytes([2; 32]),
        AccountId32::from_bytes([3; 32]),
    ];
    let task = |id| TaskView {
        task_id: id,
        reporter: AccountId32::from_bytes([4; 32]),
        created_at: UnixSeconds::new(u64::MAX),
        updated_at: UnixSeconds::new(0),
        task: definition.clone(),
    };
    let record = |storage, milestone, tasks| TaskStorageRecordV1 {
        project_id: EntityId::from_bytes([7; 16]),
        proposal_id: EntityId::from_bytes([8; 16]),
        proposal_revision: u64::MAX,
        storage: TaskStorageView {
            task_storage_id: EntityId::from_bytes([storage; 16]),
            milestone_id: EntityId::from_bytes([milestone; 16]),
            tasks,
        },
    };
    Ok(TaskStateV1 {
        schema_version: 1,
        generation: info().provider_instance_id,
        next_task_id: 9,
        storages: vec![
            record(10, 11, vec![task(4), task(1)]),
            record(9, 12, vec![]),
        ],
    })
}
async fn source(state: Arc<Mutex<TaskStateV1>>) -> Result<Server, Error> {
    Server::start(internal(fake().map_err(|_| Error::Internal)?).route(
        "/internal/task-state",
        get(move || {
            let state = state.clone();
            async move { Json(state.lock().await.clone()) }
        }),
    ))
    .await
}
async fn exported(
    app: &App,
    generation: ProviderInstanceId,
) -> Result<TaskStateV1, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("prepared-tasks-{}.json", hex_id()?));
    crate::task_state::export(app, generation, path.clone()).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let value = serde_json::from_slice(&std::fs::read(&path)?)?;
    std::fs::remove_file(path)?;
    Ok(value)
}
fn hex_id() -> Result<String, Error> {
    Ok(PrincipalId::from_bytes(crate::state::random()?).to_string())
}
fn ordered(mut state: TaskStateV1) -> TaskStateV1 {
    state.storages.sort_by_key(|r| r.storage.task_storage_id);
    state
}

#[tokio::test]
async fn task_state_backfill_preserves_counter_order_empty_storage_and_narrative() -> TestResult {
    let state = Arc::new(Mutex::new(inventory()?));
    let internal = source(state.clone()).await?;
    let app = app(&internal).await?;
    assert_eq!(crate::task_state::backfill(&app).await?, (2, 2));
    assert_eq!(crate::task_state::backfill(&app).await?, (2, 2));
    let expected = ordered(state.lock().await.clone());
    let actual = exported(&app, expected.generation).await?;
    assert_eq!(actual, expected);
    assert_eq!(actual.next_task_id, 9); // max(live IDs)+1 would incorrectly return 5.
    assert!(actual.storages[0].storage.tasks.is_empty());
    assert_eq!(
        actual.storages[1]
            .storage
            .tasks
            .iter()
            .map(|t| t.task_id)
            .collect::<Vec<_>>(),
        vec![4, 1]
    );
    // Reopen keeps exact u64 revisions/timestamps, even when outside SQL bigint.
    let url = app.config.database_url.clone();
    drop(app);
    let reopened = App::new(test_config(url, &internal)?).await?;
    assert_eq!(exported(&reopened, expected.generation).await?, expected);
    // Prepared tasks do not become public core state or confer project visibility.
    assert!(reopened.snapshot().await?.projects.is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations WHERE task_operation")
            .fetch_one(&reopened.db)
            .await?,
        0
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_state_refresh_is_generation_scoped_and_counter_never_rewinds_or_wraps() -> TestResult
{
    let state = Arc::new(Mutex::new(inventory()?));
    let internal = source(state.clone()).await?;
    let app = app(&internal).await?;
    crate::task_state::backfill(&app).await?;
    let original = exported(&app, info().provider_instance_id).await?;
    state.lock().await.next_task_id = 8;
    assert!(matches!(
        crate::task_state::backfill(&app).await,
        Err(Error::Conflict("task_counter_regressed"))
    ));
    assert_eq!(exported(&app, original.generation).await?, original);
    {
        let mut updated = state.lock().await;
        updated.next_task_id = 10;
        updated.storages[0].storage.tasks[0].task.title = "Updated source".into();
    }
    crate::task_state::backfill(&app).await?;
    assert_eq!(
        exported(&app, original.generation).await?,
        ordered(state.lock().await.clone())
    );
    state.lock().await.generation = ProviderInstanceId::from_bytes([23; 16]);
    state.lock().await.next_task_id = u32::MAX;
    crate::task_state::backfill(&app).await?;
    let second = state.lock().await.clone();
    assert_eq!(
        exported(&app, second.generation).await?,
        ordered(second.clone())
    );
    assert_eq!(exported(&app, original.generation).await?.next_task_id, 10);
    assert!(
        sqlx::query(
            "UPDATE task_generations SET next_task_id=next_task_id+1 WHERE provider_instance_id=$1"
        )
        .bind(second.generation.to_string())
        .execute(&app.db)
        .await
        .is_err()
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_state_invalid_graph_and_failed_replacement_leave_previous_data() -> TestResult {
    let state = Arc::new(Mutex::new(inventory()?));
    let internal = source(state.clone()).await?;
    let app = app(&internal).await?;
    crate::task_state::backfill(&app).await?;
    let original = exported(&app, info().provider_instance_id).await?;
    for bad in [0, 9] {
        *state.lock().await = inventory()?;
        state.lock().await.storages[0].storage.tasks[0].task_id = bad;
        assert!(crate::task_state::backfill(&app).await.is_err());
        assert_eq!(exported(&app, original.generation).await?, original);
    }
    *state.lock().await = inventory()?;
    state.lock().await.storages[0].storage.tasks[0].task_id = 1; // globally duplicated key
    assert!(crate::task_state::backfill(&app).await.is_err());
    *state.lock().await = inventory()?;
    {
        let mut duplicate = state.lock().await;
        duplicate.storages[1].storage.task_storage_id =
            duplicate.storages[0].storage.task_storage_id;
    }
    assert!(crate::task_state::backfill(&app).await.is_err());
    *state.lock().await = inventory()?;
    state.lock().await.storages[0].storage.tasks[0].task.title = "would replace old graph".into();
    sqlx::raw_sql("CREATE FUNCTION task_insert_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected task row failure'; END; $$; CREATE TRIGGER task_insert_failure BEFORE INSERT ON task_items FOR EACH ROW EXECUTE FUNCTION task_insert_failure();")
        .execute(&app.db).await?;
    assert!(crate::task_state::backfill(&app).await.is_err());
    assert_eq!(exported(&app, original.generation).await?, original);
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_state_serializes_capture_before_replacement() -> TestResult {
    let state = Arc::new(Mutex::new(inventory()?));
    let hits = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let router = internal(fake()?).route(
        "/internal/task-state",
        get({
            let state = state.clone();
            let hits = hits.clone();
            let captured = captured.clone();
            let release = release.clone();
            move || {
                let state = state.clone();
                let hits = hits.clone();
                let captured = captured.clone();
                let release = release.clone();
                async move {
                    let copy = state.lock().await.clone();
                    if hits.fetch_add(1, Ordering::SeqCst) == 0 {
                        captured.notify_one();
                        release.notified().await;
                    }
                    Json(copy)
                }
            }
        }),
    );
    let internal = Server::start(router).await?;
    let app = app(&internal).await?;
    let first = tokio::spawn({
        let app = app.clone();
        async move { crate::task_state::backfill(&app).await }
    });
    tokio::time::timeout(Duration::from_secs(5), captured.notified()).await?;
    state.lock().await.next_task_id = 10;
    let second = tokio::spawn({
        let app = app.clone();
        async move { crate::task_state::backfill(&app).await }
    });
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "second fetch must wait for first transaction"
    );
    release.notify_one();
    first.await??;
    second.await??;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(
        exported(&app, info().provider_instance_id)
            .await?
            .next_task_id,
        10
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_state_refuses_api_owned_history_and_does_not_hide_corruption() -> TestResult {
    let state = Arc::new(Mutex::new(inventory()?));
    let internal = source(state.clone()).await?;
    let app = app(&internal).await?;
    crate::task_state::backfill(&app).await?;
    sqlx::query("UPDATE task_items SET assignees=ARRAY[]::TEXT[] WHERE task_id=4")
        .execute(&app.db)
        .await?;
    assert!(crate::task_state::backfill(&app).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, Vec<String>>("SELECT assignees FROM task_items WHERE task_id=4")
            .fetch_one(&app.db)
            .await?,
        Vec::<String>::new()
    );
    sqlx::query("UPDATE task_items SET assignees=$1 WHERE task_id=4")
        .bind(vec![
            AccountId32::from_bytes([2; 32]).to_string(),
            AccountId32::from_bytes([3; 32]).to_string(),
        ])
        .execute(&app.db)
        .await?;
    let before = exported(&app, info().provider_instance_id).await?;
    let (alice, _) = register(&app, "prepared_task_owner").await?;
    let command = ProviderCommand::CreateTask {
        project_id: before.storages[0].project_id,
        task_storage_id: before.storages[0].storage.task_storage_id,
        task: before.storages[1].storage.tasks[0].task.clone(),
    };
    // Seeded future API work; this is a reseed guard, not a hidden task executor.
    sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,task_operation) VALUES($1,$2,$3,$4,$5,$6,'Queued',0,0,0,$7,TRUE)")
        .bind(OperationId::from_bytes([80;16]).to_string()).bind(alice.view.principal_id.to_string()).bind(alice.wallet.to_string())
        .bind(alice.view.account_id.to_string()).bind(serde_json::to_string(&command)?).bind(command.encode())
        .bind(before.generation.to_string()).execute(&app.db).await?;
    state.lock().await.next_task_id = 10;
    assert!(matches!(
        crate::task_state::backfill(&app).await,
        Err(Error::Conflict("task_state_api_owned"))
    ));
    assert_eq!(exported(&app, before.generation).await?, before);
    internal.finish().await?;
    Ok(())
}
