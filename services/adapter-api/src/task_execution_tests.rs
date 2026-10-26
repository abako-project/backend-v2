//! Real task execution in explicitly API-owned test cohorts. No production cutover.
use super::*;
use parity_scale_codec::Encode;
use serde_json::Value;

type CaseSlot = Arc<Mutex<Option<generated_contracts::provider_core::CoreDisputeView>>>;

struct Cohort {
    app: Arc<App>,
    internal: Server,
    fake: Arc<Mutex<Fake>>,
    owner: Session,
    owner_cookie: String,
    worker: Session,
    worker_cookie: String,
    client: Session,
    client_cookie: String,
    project: ProjectView,
    definition: TaskDefinition,
}
impl Cohort {
    async fn new(api_owned: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_case(api_owned, None).await
    }
    async fn with_case(
        api_owned: bool,
        case: Option<CaseSlot>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let fake = fake()?;
        let router = internal(fake.clone());
        let router = if let Some(case) = case {
            router.route(
                "/internal/disputes/{id}",
                get(move |Path(id): Path<String>| {
                    let case = case.clone();
                    async move {
                        let view = case.lock().await.clone().ok_or(Error::NotFound)?;
                        if view.dispute.dispute_id.to_string() != id {
                            return Err(Error::NotFound);
                        }
                        Ok::<_, Error>(Json(view))
                    }
                }),
            )
        } else {
            router
        };
        let internal = Server::start(router).await?;
        let app = app(&internal).await?;
        let (owner, owner_cookie) = register(&app, "executor_owner").await?;
        let (worker, worker_cookie) = register(&app, "executor_worker").await?;
        let (client, client_cookie) = register(&app, "executor_client").await?;
        let doc: Value = serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
        let mut project: ProjectView = serde_json::from_value(
            doc["components"]["schemas"]["ProjectView"]["examples"][0].clone(),
        )?;
        project.coordinator = owner.view.account_id;
        project.client = client.view.account_id;
        project.proposals[0].status = ProposalStatus::Draft;
        let core =
            stage_project_without_tasks(&app, info().provider_instance_id, project.clone()).await?;
        let mut known = content::source_worker();
        known.account = worker.view.account_id;
        let known = stage_worker(&app, info().provider_instance_id, known).await?;
        fake.lock().await.snapshot.projects = vec![core];
        fake.lock().await.snapshot.workers = vec![known];
        if api_owned {
            stage_task_content(&app, info().provider_instance_id, project.clone()).await?;
            sqlx::query("UPDATE task_generations SET next_task_id=9")
                .execute(&app.db)
                .await?;
        }
        let mut definition: TaskDefinition = serde_json::from_value(
            doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone(),
        )?;
        definition.assignees = vec![worker.view.account_id];
        definition.description = "λ\0".repeat(20_000);
        Ok(Self {
            app,
            internal,
            fake,
            owner,
            owner_cookie,
            worker,
            worker_cookie,
            client,
            client_cookie,
            project,
            definition,
        })
    }
    fn create(&self) -> ProviderCommand {
        ProviderCommand::CreateTask {
            project_id: self.project.project_id,
            task_storage_id: self.storage(),
            task: self.definition.clone(),
        }
    }
    fn storage(&self) -> EntityId {
        self.project.proposals[0].milestones[0]
            .task_storage
            .task_storage_id
    }
    async fn queue(&self, actor: &Session, command: ProviderCommand) -> Result<OperationId, Error> {
        let id = OperationId::from_bytes(crate::state::random()?);
        let time = now()?;
        sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,request_command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,task_operation,task_project_id) VALUES($1,$2,$3,$4,$5,$6,$6,'Queued',0,$7,$8,$9,TRUE,$10)")
            .bind(id.to_string()).bind(actor.view.principal_id.to_string()).bind(actor.wallet.to_string()).bind(actor.view.account_id.to_string())
            .bind(serde_json::to_string(&command)?).bind(command.encode()).bind(time+300).bind(time).bind(info().provider_instance_id.to_string()).bind(command.project_id().map(|id| id.to_string())).execute(&self.app.db).await?;
        Ok(id)
    }
    async fn step(&self) -> Result<(), Error> {
        let Some(operations::Work::Task(job)) = operations::claim_work(&self.app).await? else {
            return Err(Error::Internal);
        };
        crate::task_execution::process(&self.app, job).await
    }
    async fn run(&self, actor: &Session, command: ProviderCommand) -> Result<OperationView, Error> {
        let id = self.queue(actor, command).await?;
        self.finish(actor, id).await
    }
    async fn finish(&self, actor: &Session, id: OperationId) -> Result<OperationView, Error> {
        for _ in 0..16 {
            let view = operations::read(&self.app, actor, id).await?;
            if matches!(
                view.status,
                ProviderOperationStatus::Finalized
                    | ProviderOperationStatus::Rejected
                    | ProviderOperationStatus::Expired
            ) {
                return Ok(view);
            }
            core_recovery::step(&self.app).await?;
        }
        Err(Error::Internal)
    }
    async fn tasks(&self) -> Result<Vec<TaskView>, Error> {
        let payloads: Vec<String> =
            sqlx::query_scalar("SELECT payload FROM task_items ORDER BY ordinal")
                .fetch_all(&self.app.db)
                .await?;
        payloads
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Error::from))
            .collect()
    }
    async fn counter(&self) -> Result<i64, Error> {
        Ok(
            sqlx::query_scalar("SELECT next_task_id FROM task_generations")
                .fetch_one(&self.app.db)
                .await?,
        )
    }
}
fn outcome(operation: &OperationView) -> Result<&ExecutionOutcome, Box<dyn std::error::Error>> {
    assert_eq!(operation.status, ProviderOperationStatus::Finalized);
    match (&operation.receipt, &operation.task_result) {
        (Some(receipt), None) => Ok(&receipt.outcome),
        (None, Some(result)) => Ok(&result.outcome),
        _ => Err("finalized task must have exactly one result source".into()),
    }
}
#[tokio::test]
async fn task_executor_refuses_missing_generation_before_public_admission() -> TestResult {
    let c = Cohort::new(false).await?;
    let id = c.queue(&c.owner, c.create()).await?;
    assert!(operations::claim_work(&c.app).await?.is_none());
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::Queued
    );
    assert!(c.counter().await.is_err());
    assert!(c.tasks().await?.is_empty());
    sqlx::query("DELETE FROM operations WHERE operation_id=$1")
        .bind(id.to_string())
        .execute(&c.app.db)
        .await?;
    assert!(matches!(
        operations::enqueue(&c.app, &c.owner, c.create(), None).await,
        Err(Error::Conflict("task_state_missing"))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&c.app.db)
            .await?,
        0
    );
    c.internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_executor_uses_core_receipts_for_identity_and_api_results_for_progress() -> TestResult
{
    let c = Cohort::new(true).await?;
    let before = c.fake.lock().await.snapshot.clone();
    let created = c.run(&c.owner, c.create()).await?;
    assert_eq!(outcome(&created)?, &ExecutionOutcome::Success);
    let original = c.tasks().await?.remove(0);
    assert_eq!(original.task_id, 9);
    assert_eq!(c.counter().await?, 10);
    assert_eq!(original.reporter, c.owner.view.account_id);
    assert_eq!(original.task, c.definition);
    assert!(created.receipt.is_some());
    assert!(created.task_result.is_none());
    assert!(
        notifications::page(&c.app, c.worker.view.account_id, 0)
            .await?
            .notifications
            .is_empty()
    );
    let progress = TaskProgressRequest {
        status: TaskStatus::Done,
        logged_minutes: Minutes::new(125),
    };
    for actor in [&c.worker, &c.owner] {
        let updated = c
            .run(
                actor,
                ProviderCommand::UpdateTaskProgress {
                    project_id: c.project.project_id,
                    task_storage_id: c.storage(),
                    task_id: 9,
                    progress: progress.clone(),
                },
            )
            .await?;
        assert_eq!(outcome(&updated)?, &ExecutionOutcome::Success);
        assert!(updated.receipt.is_none());
        assert!(updated.task_result.is_some());
    }
    let mut edited = c.definition.clone();
    edited.title = "Edited\0title".into();
    edited.assignees.clear();
    let update = c
        .run(
            &c.owner,
            ProviderCommand::EditTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                task: edited.clone(),
            },
        )
        .await?;
    assert_eq!(outcome(&update)?, &ExecutionOutcome::Success);
    let task = c.tasks().await?.remove(0);
    assert_eq!(task.task, edited);
    assert_eq!(task.created_at, original.created_at);
    assert_eq!(task.reporter, original.reporter);
    // Removed task-only member receives neither this edit nor later events.
    assert_eq!(
        notifications::page(&c.app, c.worker.view.account_id, 0)
            .await?
            .notifications
            .len(),
        2
    );
    let deleted = c
        .run(
            &c.owner,
            ProviderCommand::DeleteTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                expected_revision: c.project.proposals[0].revision,
            },
        )
        .await?;
    assert_eq!(outcome(&deleted)?, &ExecutionOutcome::Success);
    assert!(c.tasks().await?.is_empty());
    assert_eq!(c.counter().await?, 10);
    assert_eq!(c.fake.lock().await.snapshot, before);
    assert_eq!(c.fake.lock().await.jobs.len(), 3);
    assert_eq!(
        c.fake.lock().await.nonces.get(&c.owner.view.account_id),
        Some(&3)
    );
    assert_eq!(c.fake.lock().await.submissions, 3);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM operations WHERE signable_payload IS NOT NULL OR signed_json IS NOT NULL OR receipt_json IS NOT NULL OR possibly_submitted").fetch_one(&c.app.db).await?,3);
    c.internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_executor_preserves_api_permissions_and_core_rejections() -> TestResult {
    let c = Cohort::new(true).await?;
    let bad = c.run(&c.worker, c.create()).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("coordinator_required".into())
    );
    let mut unknown = c.definition.clone();
    unknown.assignees.push(AccountId32::from_bytes([66; 32]));
    let bad = c
        .run(
            &c.owner,
            ProviderCommand::CreateTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task: unknown,
            },
        )
        .await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("unknown_task_assignee".into())
    );
    assert_eq!(c.counter().await?, 9);
    c.run(&c.owner, c.create()).await?;
    let progress = TaskProgressRequest {
        status: TaskStatus::Done,
        logged_minutes: Minutes::new(500),
    };
    let bad = c
        .run(
            &c.client,
            ProviderCommand::UpdateTaskProgress {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                progress,
            },
        )
        .await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("client_read_only".into())
    );
    let delete = |revision| ProviderCommand::DeleteTask {
        project_id: c.project.project_id,
        task_storage_id: c.storage(),
        task_id: 9,
        expected_revision: revision,
    };
    c.fake.lock().await.next_outcome = Some(ExecutionOutcome::Failed("revision_conflict".into()));
    let bad = c.run(&c.owner, delete(99)).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("revision_conflict".into())
    );
    c.fake.lock().await.snapshot.projects[0].proposals[0].status = ProposalStatus::Approved;
    c.fake.lock().await.next_outcome = Some(ExecutionOutcome::Failed("proposal_not_draft".into()));
    let bad = c.run(&c.owner, delete(1)).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("proposal_not_draft".into())
    );
    c.fake.lock().await.snapshot.projects[0].active_dispute_id =
        Some(EntityId::from_bytes([77; 16]));
    let bad = c.run(&c.owner, c.create()).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("project_disputed".into())
    );
    c.fake.lock().await.snapshot.projects[0].cancelled = true;
    let bad = c.run(&c.owner, c.create()).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    c.fake.lock().await.snapshot.projects[0].cancelled = false;
    c.fake.lock().await.snapshot.projects[0].active_dispute_id = None;
    sqlx::query("UPDATE task_generations SET next_task_id=4294967295")
        .execute(&c.app.db)
        .await?;
    c.fake.lock().await.next_outcome = Some(ExecutionOutcome::Failed("task_keys_exhausted".into()));
    let bad = c.run(&c.owner, c.create()).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("task_keys_exhausted".into())
    );
    assert_eq!(c.counter().await?, i64::from(u32::MAX));
    assert_eq!(c.tasks().await?.len(), 1);
    c.internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_executor_rolls_back_notification_failure_and_retries_exact_operation() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let edit = ProviderCommand::UpdateTaskProgress {
        project_id: c.project.project_id,
        task_storage_id: c.storage(),
        task_id: 9,
        progress: TaskProgressRequest {
            status: TaskStatus::Done,
            logged_minutes: Minutes::new(13),
        },
    };
    let original = c.tasks().await?;
    sqlx::raw_sql("CREATE FUNCTION reject_task_notification() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected notification failure'; END; $$; CREATE TRIGGER reject_task_notification BEFORE INSERT ON notifications FOR EACH ROW EXECUTE FUNCTION reject_task_notification();").execute(&c.app.db).await?;
    let id = c.queue(&c.owner, edit.clone()).await?;
    c.step().await?;
    assert_eq!(c.tasks().await?, original);
    assert_eq!(c.counter().await?, 10);
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::Queued
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_records WHERE event_kind='task_finalized'"
        )
        .fetch_one(&c.app.db)
        .await?,
        0
    );
    sqlx::raw_sql("DROP TRIGGER reject_task_notification ON notifications; UPDATE operations SET next_attempt_at=0;").execute(&c.app.db).await?;
    c.step().await?;
    let done = operations::read(&c.app, &c.owner, id).await?;
    assert_eq!(outcome(&done)?, &ExecutionOutcome::Success);
    assert_eq!(c.tasks().await?[0].task_id, 9);
    assert_eq!(c.counter().await?, 10);
    assert!(operations::claim_work(&c.app).await?.is_none());
    let replay = operations::enqueue(&c.app, &c.owner, edit, Some(id)).await?;
    assert_eq!(replay.status, ProviderOperationStatus::Finalized);
    let reopened = App::new(test_config(c.app.config.database_url.clone(), &c.internal)?).await?;
    assert_eq!(operations::read(&reopened, &c.owner, id).await?, done);
    assert_eq!(
        notifications::page(&reopened, c.worker.view.account_id, 0)
            .await?
            .notifications
            .len(),
        1
    );
    c.internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn task_executor_serializes_global_ids_and_fences_stale_leases() -> TestResult {
    let c = Cohort::new(true).await?;
    let first = c.queue(&c.owner, c.create()).await?;
    let second = c.queue(&c.owner, c.create()).await?;
    let Some(operations::Work::Task(stale)) = operations::claim_work(&c.app).await? else {
        return Err("claim".into());
    };
    assert!(operations::claim_work(&c.app).await?.is_none()); // FIFO even while the first lease is live.
    sqlx::query("UPDATE operations SET lease_until=0 WHERE operation_id=$1")
        .bind(first.to_string())
        .execute(&c.app.db)
        .await?;
    let Some(operations::Work::Task(fresh)) = operations::claim_work(&c.app).await? else {
        return Err("reclaim".into());
    };
    crate::task_execution::process(&c.app, stale).await?;
    assert!(c.tasks().await?.is_empty());
    crate::task_execution::process(&c.app, fresh).await?;
    assert_eq!(c.counter().await?, 9); // Classification cannot reserve an ID in PostgreSQL.
    c.finish(&c.owner, second).await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, first).await?)?,
        &ExecutionOutcome::Success
    );
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, second).await?)?,
        &ExecutionOutcome::Success
    );
    assert_eq!(
        c.tasks()
            .await?
            .iter()
            .map(|t| t.task_id)
            .collect::<Vec<_>>(),
        vec![9, 10]
    );
    assert_eq!(c.counter().await?, 11);
    assert_eq!(c.fake.lock().await.next_task_id, 11);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_executor_first_writes_on_two_projects_share_allocator_without_lock_upgrade()
-> TestResult {
    let c = Cohort::new(true).await?;
    let mut second = c.fake.lock().await.snapshot.projects[0].clone();
    second.coordinator = c.worker.view.account_id;
    second.project_id = EntityId::from_bytes([41; 16]);
    second.proposals[0].proposal_id = EntityId::from_bytes([42; 16]);
    second.proposals[0].milestones[0].milestone_id = EntityId::from_bytes([43; 16]);
    second.proposals[0].milestones[0].task_storage.milestone_id = EntityId::from_bytes([43; 16]);
    second.proposals[0].milestones[0]
        .task_storage
        .task_storage_id = EntityId::from_bytes([44; 16]);
    let second_project = second.project_id;
    let second_storage = second.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    c.fake.lock().await.snapshot.projects.push(second);
    let first = c.queue(&c.owner, c.create()).await?;
    let other = c
        .queue(
            &c.worker,
            ProviderCommand::CreateTask {
                project_id: second_project,
                task_storage_id: second_storage,
                task: c.definition.clone(),
            },
        )
        .await?;
    let Some(operations::Work::Task(a)) = operations::claim_work(&c.app).await? else {
        return Err("a".into());
    };
    let Some(operations::Work::Task(b)) = operations::claim_work(&c.app).await? else {
        return Err("b".into());
    };
    // Keep a legitimate FK-strength reader alive. Allocator writes must finish
    // without upgrading to a lock that conflicts with that key-share lock.
    let mut key_share = c.app.db.begin().await?;
    sqlx::query("SELECT provider_instance_id FROM task_generations FOR KEY SHARE")
        .execute(&mut *key_share)
        .await?;
    let (a, b) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            crate::task_execution::process(&c.app, a),
            crate::task_execution::process(&c.app, b)
        )
    })
    .await?;
    a?;
    b?;
    key_share.commit().await?;
    assert_eq!(c.counter().await?, 9);
    c.finish(&c.owner, first).await?;
    c.finish(&c.worker, other).await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, first).await?)?,
        &ExecutionOutcome::Success
    );
    assert_eq!(
        outcome(&operations::read(&c.app, &c.worker, other).await?)?,
        &ExecutionOutcome::Success
    );
    assert_eq!(c.counter().await?, 11);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_projects")
            .fetch_one(&c.app.db)
            .await?,
        2
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_executor_uses_core_audience_despite_corrupt_sibling_projection() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let sibling_id = EntityId::from_bytes([46; 16]);
    {
        let mut fake = c.fake.lock().await;
        let mut sibling = fake.snapshot.projects[0].proposals[0].milestones[0].clone();
        sibling.milestone_id = EntityId::from_bytes([45; 16]);
        sibling.task_storage.milestone_id = sibling.milestone_id;
        sibling.task_storage.task_storage_id = sibling_id;
        sibling.task_storage.tasks.clear();
        fake.snapshot.projects[0].proposals[0]
            .milestones
            .push(sibling);
    }
    c.run(
        &c.owner,
        ProviderCommand::CreateTask {
            project_id: c.project.project_id,
            task_storage_id: sibling_id,
            task: c.definition.clone(),
        },
    )
    .await?;
    let before: Vec<String> = sqlx::query_scalar("SELECT payload FROM task_items ORDER BY task_id")
        .fetch_all(&c.app.db)
        .await?;
    let notifications: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications")
        .fetch_one(&c.app.db)
        .await?;
    let outsider = AccountId32::from_bytes([99; 32]);
    sqlx::query("UPDATE task_items SET assignees=$1 WHERE task_storage_id=$2")
        .bind(vec![outsider.to_string()])
        .bind(sibling_id.to_string())
        .execute(&c.app.db)
        .await?;
    let mut task = c.definition.clone();
    task.title = "local edit".into();
    let id = c
        .queue(
            &c.owner,
            ProviderCommand::EditTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                task,
            },
        )
        .await?;
    c.step().await?;
    let after: Vec<String> = sqlx::query_scalar("SELECT payload FROM task_items ORDER BY task_id")
        .fetch_all(&c.app.db)
        .await?;
    assert_eq!(before[1], after[1]);
    let edited: TaskView = serde_json::from_str(&after[0])?;
    assert_eq!(edited.task.title, "local edit");
    assert_eq!(c.counter().await?, 11);
    let op = operations::read(&c.app, &c.owner, id).await?;
    assert_eq!(outcome(&op)?, &ExecutionOutcome::Success);
    assert!(op.receipt.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notifications")
            .fetch_one(&c.app.db)
            .await?,
        notifications + 3
    );
    assert!(
        notifications::page(&c.app, outsider, 0)
            .await?
            .notifications
            .is_empty()
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_executor_preserves_native_error_order_for_combined_invalid_requests() -> TestResult {
    let c = Cohort::new(true).await?;
    let missing = EntityId::from_bytes([89; 16]);
    let create = |task| ProviderCommand::CreateTask {
        project_id: c.project.project_id,
        task_storage_id: missing,
        task,
    };
    let bad = c.run(&c.worker, create(c.definition.clone())).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("coordinator_required".into())
    );
    let mut unknown = c.definition.clone();
    unknown.assignees = vec![AccountId32::from_bytes([88; 32])];
    let bad = c.run(&c.owner, create(unknown)).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("unknown_task_assignee".into())
    );
    let bad = c
        .run(
            &c.client,
            ProviderCommand::UpdateTaskProgress {
                project_id: c.project.project_id,
                task_storage_id: missing,
                task_id: 999,
                progress: TaskProgressRequest {
                    status: TaskStatus::Done,
                    logged_minutes: Minutes::new(1),
                },
            },
        )
        .await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("client_read_only".into())
    );
    let bad = c
        .run(
            &c.worker,
            ProviderCommand::DeleteTask {
                project_id: c.project.project_id,
                task_storage_id: missing,
                task_id: 999,
                expected_revision: 999,
            },
        )
        .await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("coordinator_required".into())
    );
    sqlx::query("UPDATE task_generations SET next_task_id=4294967295")
        .execute(&c.app.db)
        .await?;
    let bad = c.run(&c.owner, create(c.definition.clone())).await?;
    assert_eq!(
        outcome(&bad)?,
        &ExecutionOutcome::Failed("task_storage_not_found".into())
    );
    assert!(c.tasks().await?.is_empty());
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_executor_rechecks_ownership_generation_and_retained_request_before_commit()
-> TestResult {
    let c = Cohort::new(true).await?;
    let id = c.queue(&c.owner, c.create()).await?;
    c.fake.lock().await.snapshot.info.provider_instance_id =
        ProviderInstanceId::from_bytes([72; 16]);
    c.step().await?;
    assert!(c.tasks().await?.is_empty());
    assert_eq!(c.counter().await?, 9);
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::Queued
    );
    c.fake.lock().await.snapshot.info = info();
    sqlx::query("UPDATE operations SET command_bytes=$1,next_attempt_at=0")
        .bind(vec![0u8])
        .execute(&c.app.db)
        .await?;
    c.step().await?;
    assert!(c.tasks().await?.is_empty());
    assert_eq!(c.counter().await?, 9);
    sqlx::query(
        "UPDATE operations SET command_bytes=request_command_bytes,next_attempt_at=0,expires_at=0",
    )
    .execute(&c.app.db)
    .await?;
    c.step().await?;
    let expired = operations::read(&c.app, &c.owner, id).await?;
    assert_eq!(expired.status, ProviderOperationStatus::Expired);
    assert!(expired.task_result.is_none());
    assert!(c.tasks().await?.is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM notifications")
            .fetch_one(&c.app.db)
            .await?,
        0
    );
    c.internal.finish().await?;
    Ok(())
}

#[path = "task_freeze_tests.rs"]
mod freezes;

#[path = "task_view_tests.rs"]
mod views;

#[path = "task_core_recovery_tests.rs"]
pub(super) mod core_recovery;
