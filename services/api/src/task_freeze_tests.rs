//! Real `PostgreSQL` hold admission, task races and existing transport recovery.
use super::*;
use protocol::provider_core::TaskSnapshotRecord;

fn cancellation(project: EntityId) -> ProviderCommand {
    ProviderCommand::CancelProject {
        project_id: project,
        request: ReasonRequest {
            reason: "Cancellation λ\0".into(),
        },
    }
}
async fn freeze(c: &Cohort, actor: &Session, project: EntityId) -> Result<OperationId, Error> {
    Ok(
        operations::enqueue(&c.app, actor, cancellation(project), None)
            .await?
            .operation_id,
    )
}
async fn state(c: &Cohort, id: OperationId) -> Result<(String, String), Error> {
    let row = sqlx::query("SELECT state,snapshot_json FROM task_freezes WHERE operation_id=$1")
        .bind(id.to_string())
        .fetch_one(&c.app.db)
        .await?;
    Ok((row.try_get("state")?, row.try_get("snapshot_json")?))
}
async fn ready(c: &Cohort) -> Result<(), Error> {
    sqlx::query("UPDATE operations SET next_attempt_at=0")
        .execute(&c.app.db)
        .await?;
    Ok(())
}
async fn provider_step(c: &Cohort) -> Result<(), Error> {
    ready(c).await?;
    let Some(operations::Work::Provider(job)) = operations::claim_work(&c.app).await? else {
        return Err(Error::Internal);
    };
    operations::process(&c.app, *job).await
}
async fn finish_provider(
    c: &Cohort,
    id: OperationId,
    actor: &Session,
) -> Result<OperationView, Error> {
    for _ in 0..8 {
        let view = operations::read(&c.app, actor, id).await?;
        if matches!(
            view.status,
            ProviderOperationStatus::Finalized
                | ProviderOperationStatus::Rejected
                | ProviderOperationStatus::Expired
        ) {
            return Ok(view);
        }
        ready(c).await?;
        match operations::claim_work(&c.app)
            .await?
            .ok_or(Error::Internal)?
        {
            operations::Work::Provider(job) => operations::process(&c.app, *job).await?,
            operations::Work::Task(job) => crate::task_execution::process(&c.app, job).await?,
        }
    }
    Err(Error::Internal)
}

#[tokio::test]
async fn task_freeze_captures_confirmed_tasks_and_preserves_snapshot_on_replay() -> TestResult {
    let c = Cohort::new(true).await?;
    c.run(&c.owner, c.create()).await?;
    let expected = c.tasks().await?;
    let queued = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    let (status, snapshot) = state(&c, id).await?;
    assert_eq!(status, "Pending");
    let records: Vec<TaskSnapshotRecord> = serde_json::from_str(&snapshot)?;
    assert_eq!(records[0].storage.tasks, expected);
    assert_eq!(records[0].storage.tasks.len(), 1);
    let replay = operations::enqueue(
        &c.app,
        &c.owner,
        cancellation(c.project.project_id),
        Some(id),
    )
    .await?;
    assert_eq!(replay.operation_id, id);
    assert_eq!(state(&c, id).await?.1, snapshot);
    let mut changed = cancellation(c.project.project_id);
    if let ProviderCommand::CancelProject { request, .. } = &mut changed {
        request.reason.push_str(" conflict");
    }
    assert!(matches!(
        operations::enqueue(&c.app, &c.owner, changed, Some(id)).await,
        Err(Error::Conflict("idempotency_conflict"))
    ));
    assert_eq!(
        operations::read(&c.app, &c.owner, queued).await?.status,
        ProviderOperationStatus::Queued
    );
    finish_provider(&c, id, &c.owner).await?;
    assert_eq!(state(&c, id).await?, ("Confirmed".into(), snapshot));
    // Successful freeze is a known task failure, even after its original expiry.
    sqlx::query("UPDATE operations SET expires_at=0 WHERE operation_id=$1")
        .bind(queued.to_string())
        .execute(&c.app.db)
        .await?;
    c.step().await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, queued).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    assert_eq!(c.tasks().await?, expected);
    assert_eq!(c.counter().await?, 10);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_failed_receipt_releases_and_retains_original_snapshot() -> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    let snapshot = state(&c, id).await?.1;
    c.fake.lock().await.next_outcome = Some(ExecutionOutcome::Failed("project_completed".into()));
    finish_provider(&c, id, &c.owner).await?;
    assert_eq!(state(&c, id).await?, ("Released".into(), snapshot.clone()));
    let completed = finish_provider(&c, task, &c.owner).await?;
    assert_eq!(outcome(&completed)?, &ExecutionOutcome::Success);
    assert_eq!(c.counter().await?, 10);
    assert_eq!(state(&c, id).await?.1, snapshot);
    let captured: Vec<TaskSnapshotRecord> = serde_json::from_str(&snapshot)?;
    assert!(captured[0].storage.tasks.is_empty());
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_unsent_expiry_releases_but_waiter_does_not_get_new_lifetime() -> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    sqlx::query("UPDATE operations SET expires_at=0")
        .execute(&c.app.db)
        .await?;
    provider_step(&c).await?;
    assert_eq!(state(&c, id).await?.0, "Released");
    c.step().await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, task).await?.status,
        ProviderOperationStatus::Expired
    );
    assert_eq!(c.counter().await?, 9);
    assert!(c.tasks().await?.is_empty());
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_lost_reply_restart_reconciles_receipt_without_releasing_uncertainty()
-> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    let snapshot = state(&c, id).await?.1;
    provider_step(&c).await?;
    provider_step(&c).await?;
    c.fake.lock().await.lose_reply = true;
    provider_step(&c).await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::OutcomeUnknown
    );
    assert_eq!(state(&c, id).await?.0, "Pending");
    sqlx::query("UPDATE operations SET expires_at=0 WHERE operation_id=$1")
        .bind(task.to_string())
        .execute(&c.app.db)
        .await?;
    // Pending task is suspended even though expired; it cannot expire out of a hold.
    assert_eq!(
        operations::read(&c.app, &c.owner, task).await?.status,
        ProviderOperationStatus::Queued
    );
    assert!(c.tasks().await?.is_empty());
    let reopened = App::new(test_config(c.app.config.database_url.clone(), &c.internal)?).await?;
    let held: String = sqlx::query_scalar("SELECT state FROM task_freezes WHERE operation_id=$1")
        .bind(id.to_string())
        .fetch_one(&reopened.db)
        .await?;
    assert_eq!(held, "Pending");
    provider_step(&c).await?;
    assert_eq!(state(&c, id).await?, ("Confirmed".into(), snapshot));
    assert_eq!(c.fake.lock().await.submissions, 1);
    c.step().await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, task).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_intervening_operation_keeps_order_without_deadlocking_freeze() -> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let ordinary = operations::enqueue(
        &c.app,
        &c.owner,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Intervening".into(),
            description: "Independent".into(),
        }),
        None,
    )
    .await?
    .operation_id;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    finish_provider(&c, ordinary, &c.owner).await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::AwaitingSignature
    );
    assert_eq!(
        operations::read(&c.app, &c.owner, task).await?.status,
        ProviderOperationStatus::Queued
    );
    finish_provider(&c, id, &c.owner).await?;
    c.step().await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, task).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_rejects_missing_generation_and_leaves_planning_unchanged() -> TestResult {
    let c = Cohort::new(false).await?;
    assert!(matches!(
        freeze(&c, &c.owner, c.project.project_id).await,
        Err(Error::Dependency)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&c.app.db)
            .await?,
        0
    );
    c.internal.finish().await?;
    let c = Cohort::new(true).await?;
    let op = operations::enqueue(
        &c.app,
        &c.owner,
        ProviderCommand::DisputePlanning {
            project_id: c.project.project_id,
            request: ReasonRequest {
                reason: "Planning only".into(),
            },
        },
        None,
    )
    .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM task_freezes")
            .fetch_one(&c.app.db)
            .await?,
        0
    );
    finish_provider(&c, op.operation_id, &c.owner).await?;
    let task = c.run(&c.owner, c.create()).await?;
    assert_eq!(outcome(&task)?, &ExecutionOutcome::Success);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_cross_wallet_cycles_suspend_tasks_without_reordering_other_work() -> TestResult
{
    let c = Cohort::new(true).await?;
    let mut other = c.fake.lock().await.snapshot.projects[0].clone();
    other.project_id = EntityId::from_bytes([51; 16]);
    other.coordinator = c.client.view.account_id;
    other.client = c.owner.view.account_id;
    other.proposals[0].proposal_id = EntityId::from_bytes([52; 16]);
    other.proposals[0].milestones[0].milestone_id = EntityId::from_bytes([53; 16]);
    other.proposals[0].milestones[0].task_storage.milestone_id = EntityId::from_bytes([53; 16]);
    other.proposals[0].milestones[0]
        .task_storage
        .task_storage_id = EntityId::from_bytes([54; 16]);
    let other_project = other.project_id;
    let storage = other.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    c.fake.lock().await.snapshot.projects.push(other);
    let owner_task = c.queue(&c.owner, c.create()).await?;
    let client_task = c
        .queue(
            &c.client,
            ProviderCommand::CreateTask {
                project_id: other_project,
                task_storage_id: storage,
                task: c.definition.clone(),
            },
        )
        .await?;
    let owner_freeze = freeze(&c, &c.owner, other_project).await?;
    let client_freeze = freeze(&c, &c.client, c.project.project_id).await?;
    // A:T(P)->F(Q), B:U(Q)->G(P). Each task is held by the other wallet.
    finish_provider(&c, owner_freeze, &c.owner).await?;
    finish_provider(&c, client_freeze, &c.client).await?;
    ready(&c).await?;
    if operations::read(&c.app, &c.owner, owner_task).await?.status
        == ProviderOperationStatus::Queued
    {
        c.step().await?;
    }
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, owner_task).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    assert_eq!(
        outcome(&operations::read(&c.app, &c.client, client_task).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    assert_eq!(state(&c, owner_freeze).await?.0, "Confirmed");
    assert_eq!(state(&c, client_freeze).await?.0, "Confirmed");
    assert_eq!(c.counter().await?, 9);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_overlapping_holds_are_independent_and_confirmed_state_wins() -> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let first = freeze(&c, &c.owner, c.project.project_id).await?;
    let second = freeze(&c, &c.client, c.project.project_id).await?;
    let first_snapshot = state(&c, first).await?.1;
    let second_snapshot = state(&c, second).await?.1;
    c.fake.lock().await.next_outcome = Some(ExecutionOutcome::Failed("not_current".into()));
    finish_provider(&c, first, &c.owner).await?;
    assert_eq!(state(&c, first).await?.0, "Released");
    assert_eq!(state(&c, second).await?.0, "Pending");
    assert!(c.tasks().await?.is_empty());
    let third = freeze(&c, &c.owner, c.project.project_id).await?;
    finish_provider(&c, second, &c.client).await?;
    assert_eq!(state(&c, third).await?.0, "Pending");
    // A confirmed hold lets the earlier task finalize failure instead of waiting
    // on another pending attempt for the already frozen project.
    c.step().await?;
    assert_eq!(
        outcome(&operations::read(&c.app, &c.owner, task).await?)?,
        &ExecutionOutcome::Failed("project_cancelled".into())
    );
    assert_eq!(state(&c, first).await?, ("Released".into(), first_snapshot));
    assert_eq!(
        state(&c, second).await?,
        ("Confirmed".into(), second_snapshot)
    );
    c.internal.finish().await?;
    Ok(())
}

async fn wait_project_waiters(c: &Cohort, count: i64) -> Result<(), Error> {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity a WHERE a.wait_event_type='Lock' AND EXISTS(SELECT 1 FROM pg_locks l WHERE l.pid=a.pid AND l.relation='task_projects'::regclass)")
                .fetch_one(&c.app.db).await?;
            if waiting>=count{return Ok::<_,Error>(());}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.map_err(|_|Error::Internal)?
}

#[tokio::test]
async fn task_freeze_and_task_edit_share_project_lock_in_both_race_orders() -> TestResult {
    for capture_first in [false, true] {
        let c = Cohort::new(true).await?;
        c.run(&c.owner, c.create()).await?;
        let original = c.tasks().await?[0].task.title.clone();
        let mut edit = c.definition.clone();
        edit.title = "Committed edit".into();
        let task = c
            .queue(
                &c.owner,
                ProviderCommand::EditTask {
                    project_id: c.project.project_id,
                    task_storage_id: c.storage(),
                    task_id: 9,
                    task: edit,
                },
            )
            .await?;
        let Some(operations::Work::Task(job)) = operations::claim_work(&c.app).await? else {
            return Err("task claim".into());
        };
        if capture_first {
            sqlx::query("UPDATE operations SET expires_at=0 WHERE operation_id=$1")
                .bind(task.to_string())
                .execute(&c.app.db)
                .await?;
        }
        let mut gate = c.app.db.begin().await?;
        sqlx::query("SELECT project_id FROM task_projects WHERE provider_instance_id=$1 AND project_id=$2 FOR UPDATE")
            .bind(info().provider_instance_id.to_string()).bind(c.project.project_id.to_string()).execute(&mut *gate).await?;
        let task_future = async {
            if capture_first {
                wait_project_waiters(&c, 1).await?;
            }
            crate::task_execution::process(&c.app, job).await
        };
        let freeze_future = async {
            if !capture_first {
                wait_project_waiters(&c, 1).await?;
            }
            freeze(&c, &c.owner, c.project.project_id).await
        };
        let unlock = async {
            let ready = wait_project_waiters(&c, 2).await;
            gate.commit().await?;
            ready
        };
        let (edited, frozen, unlocked) = tokio::join!(task_future, freeze_future, unlock);
        edited?;
        let id = frozen?;
        unlocked?;
        let records: Vec<TaskSnapshotRecord> = serde_json::from_str(&state(&c, id).await?.1)?;
        let expected = if capture_first {
            original.as_str()
        } else {
            "Committed edit"
        };
        assert_eq!(records[0].storage.tasks[0].task.title, expected);
        assert_eq!(c.tasks().await?[0].task.title, expected);
        assert_eq!(
            operations::read(&c.app, &c.owner, task).await?.status,
            if capture_first {
                ProviderOperationStatus::Queued
            } else {
                ProviderOperationStatus::Finalized
            }
        );
        c.internal.finish().await?;
    }
    Ok(())
}

#[tokio::test]
async fn task_freeze_admission_failure_rolls_back_operation_reason_and_hold() -> TestResult {
    let c = Cohort::new(true).await?;
    sqlx::raw_sql("CREATE FUNCTION fail_task_capture() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected capture failure'; END; $$; CREATE TRIGGER fail_task_capture BEFORE INSERT ON task_freezes FOR EACH ROW EXECUTE FUNCTION fail_task_capture();").execute(&c.app.db).await?;
    let id = OperationId::from_bytes([77; 16]);
    assert!(
        operations::enqueue(
            &c.app,
            &c.owner,
            cancellation(c.project.project_id),
            Some(id)
        )
        .await
        .is_err()
    );
    for table in ["operations", "task_freezes", "lifecycle_reason_documents"] {
        let count: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&c.app.db)
                .await?;
        assert_eq!(count, 0);
    }
    sqlx::raw_sql("DROP TRIGGER fail_task_capture ON task_freezes;")
        .execute(&c.app.db)
        .await?;
    operations::enqueue(
        &c.app,
        &c.owner,
        cancellation(c.project.project_id),
        Some(id),
    )
    .await?;
    assert_eq!(state(&c, id).await?.0, "Pending");
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_milestone_dispute_commits_opening_and_projects_task_failure() -> TestResult {
    for written in [false, true] {
        let c = Cohort::new(true).await?;
        let task = c.queue(&c.owner, c.create()).await?;
        let submission = EntityId::from_bytes([81; 16]);
        let comment = EntityId::from_bytes([82; 16]);
        let milestone = c.project.proposals[0].milestones[0].milestone_id;
        {
            let mut fake = c.fake.lock().await;
            let target = &mut fake.snapshot.projects[0].proposals[0].milestones[0];
            target.status = Some(MilestoneStatus::ChangesRequested);
            target.submissions = vec![CompletionSubmission {
                submission_id: submission,
                version: 1,
                deliverable: None,
                submitted_by: c.owner.view.account_id,
                submitted_at: UnixSeconds::new(100),
                worker_ratings: vec![],
                review: SubmissionReview::RejectedWithComment {
                    comment_id: comment,
                    reviewed_by: c.client.view.account_id,
                    reviewed_at: UnixSeconds::new(101),
                },
            }];
        }
        let request = if written {
            sqlx::query("INSERT INTO dispute_opening_arguments(provider_instance_id,comment_id,project_id,milestone_id,submission_id,author_account,reason) VALUES($1,$2,$3,$4,$5,$6,'Written opening')")
                .bind(info().provider_instance_id.to_string()).bind(comment.to_string()).bind(c.project.project_id.to_string()).bind(milestone.to_string()).bind(submission.to_string()).bind(c.owner.view.account_id.to_string()).execute(&c.app.db).await?;
            ProviderCommand::OpenDisputeWithComment(OpenDisputeWithCommentRequest {
                project_id: c.project.project_id,
                milestone_id: milestone,
                rejected_submission_id: submission,
                comment_id: comment,
            })
        } else {
            ProviderCommand::OpenDispute(OpenDisputeRequest {
                project_id: c.project.project_id,
                milestone_id: milestone,
                rejected_submission_id: submission,
                evidence: EvidenceReference::new("https://example.test/opening".into())?,
            })
        };
        let id = operations::enqueue(&c.app, &c.owner, request, None)
            .await?
            .operation_id;
        assert_eq!(state(&c, id).await?.0, "Pending");
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT kind FROM task_freezes WHERE operation_id=$1")
                .bind(id.to_string())
                .fetch_one(&c.app.db)
                .await?,
            "Dispute"
        );
        finish_provider(&c, id, &c.owner).await?;
        c.step().await?;
        assert_eq!(
            outcome(&operations::read(&c.app, &c.owner, task).await?)?,
            &ExecutionOutcome::Failed("project_disputed".into())
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM evidence_url_records")
                .fetch_one(&c.app.db)
                .await?,
            i64::from(!written)
        );
        c.internal.finish().await?;
    }
    Ok(())
}

#[tokio::test]
async fn task_freeze_expired_uncertain_submission_never_releases_without_receipt() -> TestResult {
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    provider_step(&c).await?;
    provider_step(&c).await?;
    c.fake.lock().await.fail_before_execution = true;
    provider_step(&c).await?;
    sqlx::query("UPDATE operations SET expires_at=0")
        .execute(&c.app.db)
        .await?;
    for _ in 0..3 {
        provider_step(&c).await?;
        assert_eq!(state(&c, id).await?.0, "Pending");
        assert_eq!(
            operations::read(&c.app, &c.owner, id).await?.status,
            ProviderOperationStatus::OutcomeUnknown
        );
        assert_eq!(
            operations::read(&c.app, &c.owner, task).await?.status,
            ProviderOperationStatus::Queued
        );
    }
    assert_eq!(c.counter().await?, 9);
    assert!(c.tasks().await?.is_empty());
    assert!(c.fake.lock().await.receipts.is_empty());
    let reopened = App::new(test_config(c.app.config.database_url.clone(), &c.internal)?).await?;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM task_freezes")
            .fetch_one(&reopened.db)
            .await?,
        "Pending"
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_explicit_first_http_rejection_releases_despite_submission_marker() -> TestResult
{
    let c = Cohort::new(true).await?;
    let task = c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    provider_step(&c).await?;
    provider_step(&c).await?;
    c.fake.lock().await.rejection_status = Some(StatusCode::BAD_REQUEST);
    provider_step(&c).await?;
    let view = operations::read(&c.app, &c.owner, id).await?;
    assert_eq!(view.status, ProviderOperationStatus::Rejected);
    assert_eq!(view.error_code.as_deref(), Some("provider_rejected"));
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT possibly_submitted FROM operations WHERE operation_id=$1"
        )
        .bind(id.to_string())
        .fetch_one(&c.app.db)
        .await?
    );
    assert_eq!(state(&c, id).await?.0, "Released");
    c.fake.lock().await.rejection_status = None;
    let completed = finish_provider(&c, task, &c.owner).await?;
    assert_eq!(outcome(&completed)?, &ExecutionOutcome::Success);
    assert!(!c.fake.lock().await.receipts.contains_key(&id));
    assert!(c.fake.lock().await.receipts.contains_key(&task));
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_stale_submission_lease_cannot_overwrite_confirmed_hold() -> TestResult {
    let c = Cohort::new(true).await?;
    c.queue(&c.owner, c.create()).await?;
    let id = freeze(&c, &c.owner, c.project.project_id).await?;
    let snapshot = state(&c, id).await?.1;
    provider_step(&c).await?;
    provider_step(&c).await?;
    let Some(operations::Work::Provider(stale)) = operations::claim_work(&c.app).await? else {
        return Err("stale".into());
    };
    sqlx::query("UPDATE operations SET lease_until=0 WHERE operation_id=$1")
        .bind(id.to_string())
        .execute(&c.app.db)
        .await?;
    let Some(operations::Work::Provider(fresh)) = operations::claim_work(&c.app).await? else {
        return Err("fresh".into());
    };
    operations::process(&c.app, *fresh).await?;
    assert_eq!(state(&c, id).await?.0, "Confirmed");
    operations::process(&c.app, *stale).await?;
    assert_eq!(state(&c, id).await?, ("Confirmed".into(), snapshot));
    assert_eq!(c.fake.lock().await.submissions, 1);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn task_freeze_finality_sql_failure_rolls_back_and_reconciles_without_resubmission()
-> TestResult {
    for success in [false, true] {
        let c = Cohort::new(true).await?;
        c.run(&c.owner, c.create()).await?;
        let task = c.queue(&c.owner, c.create()).await?;
        let id = freeze(&c, &c.owner, c.project.project_id).await?;
        let snapshot = state(&c, id).await?.1;
        provider_step(&c).await?;
        provider_step(&c).await?;
        if !success {
            c.fake.lock().await.next_outcome =
                Some(ExecutionOutcome::Failed("project_completed".into()));
        }
        sqlx::raw_sql("CREATE FUNCTION fail_freeze_finality() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected finality failure'; END; $$; CREATE TRIGGER fail_freeze_finality BEFORE UPDATE ON task_freezes FOR EACH ROW EXECUTE FUNCTION fail_freeze_finality();").execute(&c.app.db).await?;
        assert!(provider_step(&c).await.is_err());
        assert_eq!(state(&c, id).await?, ("Pending".into(), snapshot.clone()));
        let view = operations::read(&c.app, &c.owner, id).await?;
        assert_eq!(view.status, ProviderOperationStatus::Submitted);
        assert!(view.receipt.is_none());
        assert_eq!(c.fake.lock().await.submissions, 2);
        assert!(c.fake.lock().await.receipts.contains_key(&id));
        sqlx::raw_sql("DROP TRIGGER fail_freeze_finality ON task_freezes;")
            .execute(&c.app.db)
            .await?;
        sqlx::query("UPDATE operations SET lease_until=0 WHERE operation_id=$1")
            .bind(id.to_string())
            .execute(&c.app.db)
            .await?;
        let reopened =
            App::new(test_config(c.app.config.database_url.clone(), &c.internal)?).await?;
        let Some(operations::Work::Provider(recovery)) = operations::claim_work(&reopened).await?
        else {
            return Err("recovery claim".into());
        };
        operations::process(&reopened, *recovery).await?;
        assert_eq!(
            operations::read(&reopened, &c.owner, id).await?.status,
            ProviderOperationStatus::Finalized
        );
        assert_eq!(
            state(&c, id).await?,
            (
                if success { "Confirmed" } else { "Released" }.into(),
                snapshot
            )
        );
        assert_eq!(c.fake.lock().await.submissions, 2);
        c.finish(&c.owner, task).await?;
        assert_eq!(
            outcome(&operations::read(&c.app, &c.owner, task).await?)?,
            &if success {
                ExecutionOutcome::Failed("project_cancelled".into())
            } else {
                ExecutionOutcome::Success
            }
        );
        c.internal.finish().await?;
    }
    Ok(())
}
