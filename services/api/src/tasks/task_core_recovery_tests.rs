use super::*;
// Transport fixtures supply successful core facts; the native/WASM signed tests
// independently exercise contract authorization, allocation and event behavior.
pub(in crate::tests) fn apply_fixture(
    fake: &mut Fake,
    call: &UnsignedContractCallV1,
    mut receipt: OperationReceipt,
) -> Result<OperationReceipt, Error> {
    use protocol::provider_core::{TaskAllocation, TaskCore};
    if receipt.outcome != ExecutionOutcome::Success {
        return Ok(receipt);
    }
    let (project_id, storage_id) = match &call.command {
        ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id,
            ..
        }
        | ProviderCommand::SetTaskAssigneesCore {
            project_id,
            task_storage_id,
            ..
        }
        | ProviderCommand::DeleteTask {
            project_id,
            task_storage_id,
            ..
        } => (*project_id, *task_storage_id),
        _ => return Ok(receipt),
    };
    let Some(storage) = fake
        .snapshot
        .projects
        .iter_mut()
        .find(|p| p.project_id == project_id)
        .and_then(|p| {
            p.proposals
                .iter_mut()
                .flat_map(|p| &mut p.milestones)
                .find(|m| m.task_storage.task_storage_id == storage_id)
        })
        .map(|m| &mut m.task_storage)
    else {
        return Err(Error::Dependency);
    };
    receipt.created_entity_id = Some(storage_id);
    match &call.command {
        ProviderCommand::CreateTaskCore { assignees, .. } => {
            let task_id = fake.next_task_id;
            fake.next_task_id = task_id.checked_add(1).ok_or(Error::Dependency)?;
            storage.tasks.push(TaskCore {
                task_id,
                assignees: assignees.clone(),
            });
            fake.task_allocations.insert(
                call.operation_id,
                TaskAllocation {
                    receipt: receipt.clone(),
                    project_id,
                    task_storage_id: storage_id,
                    task_id,
                },
            );
        }
        ProviderCommand::SetTaskAssigneesCore {
            task_id, assignees, ..
        } => {
            storage
                .tasks
                .iter_mut()
                .find(|t| t.task_id == *task_id)
                .ok_or(Error::Dependency)?
                .assignees
                .clone_from(assignees);
        }
        ProviderCommand::DeleteTask { task_id, .. } => {
            storage.tasks.retain(|t| t.task_id != *task_id);
        }
        _ => unreachable!(),
    }
    Ok(receipt)
}

pub(super) async fn step(app: &App) -> Result<(), Error> {
    match operations::claim_work(app).await?.ok_or(Error::Internal)? {
        operations::Work::Task(job) => crate::tasks::execution::process(app, job).await,
        operations::Work::Provider(job) => operations::process(app, *job).await,
    }
}
async fn ready(c: &Cohort, id: OperationId) -> TestResult {
    // Classification, payload preparation and wallet signing are distinct claims.
    for _ in 0..3 {
        step(&c.app).await?;
    }
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::ReadyToSubmit
    );
    Ok(())
}
async fn retry(app: &App, id: OperationId) -> Result<(), Error> {
    sqlx::query("UPDATE operations SET next_attempt_at=0,lease_until=CASE WHEN lease_token IS NULL THEN NULL ELSE 0 END WHERE operation_id=$1")
        .bind(id.to_string()).execute(&app.db).await?;
    step(app).await
}

#[tokio::test]
async fn allocation_outage_retains_fifo_and_recovers_after_restart_expiry_and_lifecycle_change()
-> TestResult {
    let c = Cohort::new(true).await?;
    let id = c.queue(&c.owner, c.create()).await?;
    ready(&c, id).await?;
    c.fake.lock().await.allocation_unavailable = true;
    step(&c.app).await?;
    let uncertain = operations::read(&c.app, &c.owner, id).await?;
    assert_eq!(uncertain.status, ProviderOperationStatus::OutcomeUnknown);
    assert!(c.tasks().await?.is_empty());
    let later = c
        .queue(
            &c.worker,
            ProviderCommand::UpdateTaskProgress {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                progress: TaskProgressRequest {
                    status: TaskStatus::Done,
                    logged_minutes: Minutes::new(5),
                },
            },
        )
        .await?;
    assert!(operations::claim_work(&c.app).await?.is_none());
    assert_eq!(
        operations::read(&c.app, &c.worker, later).await?.status,
        ProviderOperationStatus::Queued
    );
    sqlx::query("UPDATE operations SET expires_at=0 WHERE operation_id=$1")
        .bind(id.to_string())
        .execute(&c.app.db)
        .await?;
    {
        let mut fake = c.fake.lock().await;
        fake.allocation_unavailable = false;
        fake.snapshot.projects[0].coordinator = c.client.view.account_id;
        fake.snapshot.projects[0].proposals[0].status = ProposalStatus::Approved;
    }
    // Reopen the same API database and discard all in-memory operation state.
    let restarted = App::new(test_config(c.app.config.database_url.clone(), &c.internal)?).await?;
    retry(&restarted, id).await?;
    let complete = operations::read(&restarted, &c.owner, id).await?;
    assert_eq!(complete.status, ProviderOperationStatus::Finalized);
    assert!(complete.task_result.is_none());
    assert_eq!(
        complete.receipt.as_ref().ok_or("receipt")?.outcome,
        ExecutionOutcome::Success
    );
    assert_eq!(c.tasks().await?[0].task, c.definition);
    assert_eq!(c.tasks().await?[0].task_id, 9);
    assert_eq!(c.counter().await?, 10);
    assert_eq!(c.fake.lock().await.submissions, 1);
    assert_eq!(c.fake.lock().await.next_task_id, 10);
    assert!(
        notifications::page(&c.app, c.owner.view.account_id, 0)
            .await?
            .notifications
            .is_empty()
    );
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn materialization_sql_failure_is_atomic_and_reconciliation_does_not_execute_twice()
-> TestResult {
    let c = Cohort::new(true).await?;
    let id = c.queue(&c.owner, c.create()).await?;
    ready(&c, id).await?;
    sqlx::query(
        "ALTER TABLE task_items ADD CONSTRAINT test_fail_materialization CHECK(task_id<>9)",
    )
    .execute(&c.app.db)
    .await?;
    assert!(step(&c.app).await.is_err());
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::Submitted
    );
    assert!(c.tasks().await?.is_empty());
    assert_eq!(c.counter().await?, 9);
    assert_eq!(c.fake.lock().await.receipts.len(), 1);
    sqlx::query("ALTER TABLE task_items DROP CONSTRAINT test_fail_materialization")
        .execute(&c.app.db)
        .await?;
    retry(&c.app, id).await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, id).await?.status,
        ProviderOperationStatus::Finalized
    );
    assert_eq!(c.tasks().await?.len(), 1);
    assert_eq!(c.counter().await?, 10);
    assert_eq!(c.fake.lock().await.submissions, 1);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn stale_task_lease_cannot_convert_or_overtake_another_wallet_in_the_same_project()
-> TestResult {
    let c = Cohort::new(true).await?;
    let first = c.queue(&c.owner, c.create()).await?;
    let later = c.queue(&c.worker, c.create()).await?;
    let operations::Work::Task(stale) = operations::claim_work(&c.app).await?.ok_or("claim")?
    else {
        return Err("wrong source".into());
    };
    assert!(operations::claim_work(&c.app).await?.is_none());
    sqlx::query("UPDATE operations SET lease_until=0 WHERE operation_id=$1")
        .bind(first.to_string())
        .execute(&c.app.db)
        .await?;
    let operations::Work::Task(current) = operations::claim_work(&c.app).await?.ok_or("reclaim")?
    else {
        return Err("wrong source".into());
    };
    crate::tasks::execution::process(&c.app, stale).await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, first).await?.status,
        ProviderOperationStatus::Queued
    );
    crate::tasks::execution::process(&c.app, current).await?;
    assert_eq!(
        operations::read(&c.app, &c.owner, first).await?.status,
        ProviderOperationStatus::AwaitingSignature
    );
    assert_eq!(
        operations::read(&c.app, &c.worker, later).await?.status,
        ProviderOperationStatus::Queued
    );
    assert!(c.fake.lock().await.jobs.is_empty());
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn missing_task_preimage_prevents_membership_submission() -> TestResult {
    let c = Cohort::new(true).await?;
    let created = c.queue(&c.owner, c.create()).await?;
    ready(&c, created).await?;
    step(&c.app).await?;
    sqlx::query("DELETE FROM task_items WHERE task_id=9")
        .execute(&c.app.db)
        .await?;
    let mut definition = c.definition.clone();
    definition.assignees.clear();
    let edit = c
        .queue(
            &c.owner,
            ProviderCommand::EditTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                task: definition,
            },
        )
        .await?;
    step(&c.app).await?;
    let row =
        sqlx::query("SELECT task_operation,signable_payload FROM operations WHERE operation_id=$1")
            .bind(edit.to_string())
            .fetch_one(&c.app.db)
            .await?;
    assert!(row.try_get::<bool, _>("task_operation")?);
    assert!(
        row.try_get::<Option<Vec<u8>>, _>("signable_payload")?
            .is_none()
    );
    assert_eq!(c.fake.lock().await.submissions, 1);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn uncertain_core_intents_block_freeze_admission_atomically() -> TestResult {
    let c = Cohort::new(true).await?;
    let id = c.queue(&c.owner, c.create()).await?;
    step(&c.app).await?;
    for (status, error) in [
        ("Expired", None),
        ("Rejected", Some("provider_unavailable")),
        ("Finalized", None),
    ] {
        sqlx::query("UPDATE operations SET status=$2,possibly_submitted=TRUE,error_code=$3 WHERE operation_id=$1")
            .bind(id.to_string()).bind(status).bind(error).execute(&c.app.db).await?;
        let request = ProviderCommand::CancelProject {
            project_id: c.project.project_id,
            request: ReasonRequest {
                reason: "cancel".into(),
            },
        };
        assert!(matches!(
            operations::enqueue(&c.app, &c.owner, request, None).await,
            Err(Error::Conflict("task_core_pending"))
        ));
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM operations), (SELECT count(*) FROM task_freezes)",
        )
        .fetch_one(&c.app.db)
        .await?;
        assert_eq!(counts, (1, 0));
    }
    sqlx::query("UPDATE operations SET status='Rejected',error_code='provider_rejected' WHERE operation_id=$1")
        .bind(id.to_string()).execute(&c.app.db).await?;
    operations::enqueue(
        &c.app,
        &c.owner,
        ProviderCommand::CancelProject {
            project_id: c.project.project_id,
            request: ReasonRequest {
                reason: "cancel".into(),
            },
        },
        None,
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_freezes")
        .fetch_one(&c.app.db)
        .await?;
    assert_eq!(count, 1);
    c.internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn missing_local_task_content_defers_edit_and_progress_until_the_same_operation_recovers()
-> TestResult {
    for progress in [false, true] {
        let c = Cohort::new(true).await?;
        c.run(&c.owner, c.create()).await?;
        let saved: String = sqlx::query_scalar(
            "DELETE FROM task_items t WHERE task_id=9 RETURNING row_to_json(t)::text",
        )
        .fetch_one(&c.app.db)
        .await?;
        let command = if progress {
            ProviderCommand::UpdateTaskProgress {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                progress: TaskProgressRequest {
                    status: TaskStatus::InProgress,
                    logged_minutes: Minutes::new(12),
                },
            }
        } else {
            let mut task = c.definition.clone();
            task.title = "Recovered edit".into();
            ProviderCommand::EditTask {
                project_id: c.project.project_id,
                task_storage_id: c.storage(),
                task_id: 9,
                task,
            }
        };
        let actor = if progress { &c.worker } else { &c.owner };
        let id = c.queue(actor, command).await?;
        step(&c.app).await?;
        let pending = operations::read(&c.app, actor, id).await?;
        assert_eq!(pending.status, ProviderOperationStatus::Queued);
        assert!(pending.task_result.is_none());
        assert!(pending.receipt.is_none());
        let untouched: bool = sqlx::query_scalar("SELECT task_operation AND signable_payload IS NULL AND signed_json IS NULL AND receipt_json IS NULL AND task_result_json IS NULL FROM operations WHERE operation_id=$1")
            .bind(id.to_string()).fetch_one(&c.app.db).await?;
        assert!(untouched);
        assert_eq!(
            c.fake.lock().await.submissions,
            1,
            "only initial core creation"
        );
        let notifications: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notifications WHERE api_operation_id=$1")
                .bind(id.to_string())
                .fetch_one(&c.app.db)
                .await?;
        assert_eq!(notifications, 0);
        sqlx::query(
            "INSERT INTO task_items SELECT * FROM json_populate_record(NULL::task_items,$1::json)",
        )
        .bind(saved)
        .execute(&c.app.db)
        .await?;
        retry(&c.app, id).await?;
        let completed = operations::read(&c.app, actor, id).await?;
        assert_eq!(outcome(&completed)?, &ExecutionOutcome::Success);
        assert!(completed.receipt.is_none());
        assert_eq!(c.fake.lock().await.submissions, 1);
        c.internal.finish().await?;
    }
    Ok(())
}
