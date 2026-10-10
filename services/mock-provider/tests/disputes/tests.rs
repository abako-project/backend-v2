use super::*;

async fn submit(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    project_id: EntityId,
    milestone_id: EntityId,
) -> TestResult<EntityId> {
    let receipt = success(
        provider,
        coordinator,
        ProviderCommand::RequestMilestoneCompletion {
            project_id,
            milestone_id,
            request: RequestMilestoneCompletionRequest {
                deliverable: evidence()?,
                worker_ratings: vec![WorkerRating {
                    worker: account(worker),
                    score: Score::new(4)?,
                }],
            },
        },
    )
    .await?;
    receipt
        .created_entity_id
        .ok_or_else(|| "missing submission ID".into())
}

fn reject(
    project_id: EntityId,
    milestone_id: EntityId,
    submission_id: EntityId,
) -> TestResult<ProviderCommand> {
    Ok(ProviderCommand::RejectMilestoneCompletion {
        project_id,
        milestone_id,
        submission_id,
        request: EvidenceRequest {
            evidence: evidence()?,
        },
    })
}

async fn prepare_project(
    provider: &Provider,
) -> TestResult<(
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    EntityId,
    EntityId,
    EntityId,
)> {
    let (root, coordinator, worker, client) = setup(provider).await?;
    success(
        provider,
        &worker,
        ProviderCommand::SetCalendar(capacity(600)),
    )
    .await?;
    let mut definition = proposal()?;
    let mut second = definition.milestones[0].clone();
    second.key = 2;
    definition.milestones.push(second);
    let (project_id, proposal_id) =
        delivered_plan(provider, &coordinator, &client, definition).await?;
    assert_eq!(
        approve(provider, &client, project_id, proposal_id)
            .await?
            .outcome,
        ExecutionOutcome::Success
    );
    let milestone_id = project(provider, project_id).await?.proposals[0].milestones[0].milestone_id;
    Ok((
        root,
        coordinator,
        worker,
        client,
        project_id,
        proposal_id,
        milestone_id,
    ))
}
async fn exercise(provider: Provider) -> TestResult {
    let (root, coordinator, worker, client, project_id, proposal_id, milestone_id) =
        prepare_project(&provider).await?;
    let first = submit(&provider, &coordinator, &worker, project_id, milestone_id).await?;
    let opening = |id| -> TestResult<ProviderCommand> {
        Ok(ProviderCommand::OpenDispute(OpenDisputeRequest {
            project_id,
            milestone_id,
            rejected_submission_id: id,
            evidence: evidence()?,
        }))
    };
    assert_eq!(
        send(&provider, &client, opening(first)?).await?.outcome,
        ExecutionOutcome::Failed("milestone_not_changes_requested".into())
    );
    success(&provider, &client, reject(project_id, milestone_id, first)?).await?;
    let second = submit(&provider, &coordinator, &worker, project_id, milestone_id).await?;
    assert_eq!(
        send(&provider, &client, reject(project_id, milestone_id, first)?)
            .await?
            .outcome,
        ExecutionOutcome::Failed("submission_not_current".into())
    );
    assert_eq!(
        send(
            &provider,
            &client,
            ProviderCommand::AcceptMilestoneCompletion {
                project_id,
                milestone_id,
                request: AcceptMilestoneCompletionRequest {
                    submission_id: first,
                    coordinator_score: Score::new(4)?,
                    team_rating: TeamRating::DelegateToCoordinator,
                },
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("submission_not_current".into())
    );
    success(
        &provider,
        &client,
        reject(project_id, milestone_id, second)?,
    )
    .await?;
    assert_eq!(
        send(&provider, &client, opening(first)?).await?.outcome,
        ExecutionOutcome::Failed("submission_not_current".into())
    );
    for outsider in [&root, &worker] {
        assert_eq!(
            send(&provider, outsider, opening(second)?).await?.outcome,
            ExecutionOutcome::Failed("project_party_required".into())
        );
    }
    let before = provider.snapshot().await?;
    let left = signed(&provider, &client, opening(second)?).await?;
    let right = signed(&provider, &coordinator, opening(second)?).await?;
    let (a, b) = tokio::join!(
        provider.execute(left.clone(), NOW),
        provider.execute(right.clone(), NOW)
    );
    let (a, b) = (a?, b?);
    assert_eq!(
        usize::from(a.outcome == ExecutionOutcome::Success)
            + usize::from(b.outcome == ExecutionOutcome::Success),
        1
    );
    let (winner, signed_winner, responder) = if a.outcome == ExecutionOutcome::Success {
        (a, left, &coordinator)
    } else {
        (b, right, &client)
    };
    let id = winner.created_entity_id.ok_or("missing dispute ID")?;
    assert_eq!(provider.execute(signed_winner, NOW).await?, winner);
    let frozen = provider.snapshot().await?;
    assert_eq!(before.balances, frozen.balances);
    assert_eq!(before.workers, frozen.workers);
    let case = provider.dispute(id).await?;
    assert_eq!(case.dispute.rejected_submission_id, second);
    assert_eq!(case.milestone.submissions.len(), 2);
    assert_eq!(case.milestone.status, Some(MilestoneStatus::Disputed));
    assert_frozen(&provider, &coordinator, &client, project_id, proposal_id).await?;
    assert_response(&provider, &worker, responder, project_id, id, &frozen).await?;
    Ok(())
}
async fn assert_response(
    provider: &Provider,
    worker: &Keypair,
    responder: &Keypair,
    project_id: EntityId,
    dispute_id: EntityId,
    frozen: &ProviderSnapshot,
) -> TestResult {
    let response = ProviderCommand::RespondDispute {
        project_id,
        dispute_id,
        request: EvidenceRequest {
            evidence: evidence()?,
        },
    };
    assert_eq!(
        send(provider, worker, response.clone()).await?.outcome,
        ExecutionOutcome::Failed("dispute_response_forbidden".into())
    );
    let signed_response = signed(provider, responder, response.clone()).await?;
    let receipt = provider.execute(signed_response.clone(), NOW).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(provider.execute(signed_response, NOW).await?, receipt);
    assert_eq!(
        send(provider, responder, response).await?.outcome,
        ExecutionOutcome::Failed("dispute_already_answered".into())
    );
    assert_eq!(&provider.snapshot().await?, frozen);
    assert!(
        provider
            .dispute(dispute_id)
            .await?
            .dispute
            .response
            .is_some()
    );
    Ok(())
}

async fn assert_frozen(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
    project_id: EntityId,
    proposal_id: EntityId,
) -> TestResult {
    let before = provider.snapshot().await?;
    let view = project(provider, project_id).await?;
    let milestone = &view.proposals[0].milestones[1];
    let task = TaskDefinition {
        title: "Blocked".into(),
        description: String::new(),
        task_type: TaskType::Task,
        priority: TaskPriority::Low,
        status: TaskStatus::ToDo,
        assignees: vec![],
        estimated_minutes: Minutes::ZERO,
        logged_minutes: Minutes::ZERO,
        due_at: None,
    };
    let commands = vec![
        ProviderCommand::CreateTask {
            project_id,
            task_storage_id: milestone.task_storage.task_storage_id,
            task: task.clone(),
        },
        ProviderCommand::EditTask {
            project_id,
            task_storage_id: milestone.task_storage.task_storage_id,
            task_id: 1,
            task,
        },
        ProviderCommand::UpdateTaskProgress {
            project_id,
            task_storage_id: milestone.task_storage.task_storage_id,
            task_id: 1,
            progress: TaskProgressRequest {
                status: TaskStatus::Done,
                logged_minutes: Minutes::new(1),
            },
        },
        ProviderCommand::CreateProposal {
            project_id,
            proposal: proposal()?,
        },
        ProviderCommand::UpdateProposal {
            project_id,
            proposal_id,
            proposal: proposal()?,
        },
        ProviderCommand::DeleteProposal {
            project_id,
            proposal_id,
        },
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
        ProviderCommand::ApproveExecution {
            project_id,
            proposal_id,
            expected_revision: 0,
        },
        ProviderCommand::RequestProposalChanges {
            project_id,
            proposal_id,
            request: RequestChangesRequest {
                reference: "blocked".into(),
            },
        },
        ProviderCommand::CancelProject {
            project_id,
            request: ReasonRequest {
                reason: "blocked".into(),
            },
        },
        ProviderCommand::DisputePlanning {
            project_id,
            request: ReasonRequest {
                reason: "blocked".into(),
            },
        },
        ProviderCommand::AcceptPlanningQuote {
            project_id,
            expected_revision: 0,
        },
        ProviderCommand::AcceptPlanningDelivery {
            project_id,
            expected_revision: 0,
        },
    ];
    for command in commands {
        for actor in [coordinator, client] {
            assert_eq!(
                send(provider, actor, command.clone()).await?.outcome,
                ExecutionOutcome::Failed("project_disputed".into())
            );
        }
    }
    assert_eq!(provider.snapshot().await?, before);
    Ok(())
}

#[cfg(feature = "storage-memory")]
#[tokio::test]
async fn memory_rejection_freeze_response_and_replay() -> TestResult {
    exercise(Provider::memory(account(&key(1)?))?).await
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn sqlite_rejection_freeze_response_and_restore() -> TestResult {
    let file = std::env::temp_dir().join(format!(
        "kunveno-disputes-{}-{}.sqlite",
        std::process::id(),
        OPERATIONS.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    assert!(!file.exists());
    exercise(Provider::sqlite(&url, account(&key(1)?)).await?).await?;
    let restored = Provider::sqlite(&url, account(&key(1)?)).await?;
    let view = restored
        .snapshot()
        .await?
        .projects
        .into_iter()
        .find(|p| p.active_dispute_id.is_some())
        .ok_or("missing frozen project")?;
    let id = view.active_dispute_id.ok_or("missing dispute")?;
    assert!(restored.dispute(id).await?.dispute.response.is_some());
    assert_eq!(
        send(
            &restored,
            &key(4)?,
            ProviderCommand::CancelProject {
                project_id: view.project_id,
                request: ReasonRequest {
                    reason: "blocked".into()
                }
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("project_disputed".into())
    );
    drop(restored);
    std::fs::remove_file(file)?;
    Ok(())
}

#[cfg(feature = "storage-memory")]
#[tokio::test]
async fn delivery_without_links_still_checks_actor_ratings_and_conserves_escrow() -> TestResult {
    let provider = Provider::memory(account(&key(1)?))?;
    let (_, coordinator, worker, client, project_id, _, milestone_id) =
        prepare_project(&provider).await?;
    let before = provider.snapshot().await?;
    let command = ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
        project_id,
        milestone_id,
        worker_ratings: vec![WorkerRating {
            worker: account(&worker),
            score: Score::new(4)?,
        }],
    };
    assert_eq!(
        send(&provider, &worker, command.clone()).await?.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    let invalid = ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
        project_id,
        milestone_id,
        worker_ratings: vec![],
    };
    assert_eq!(
        send(&provider, &coordinator, invalid).await?.outcome,
        ExecutionOutcome::Failed("worker_ratings_mismatch".into())
    );
    let signed = signed(&provider, &coordinator, command).await?;
    let receipt = provider.execute(signed.clone(), NOW).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(provider.execute(signed, NOW).await?, receipt);
    let after = provider.snapshot().await?;
    assert_eq!(before.balances, after.balances);
    assert_eq!(before.workers, after.workers);
    let view = project(&provider, project_id).await?;
    assert_eq!(view.execution_escrow, before.projects[0].execution_escrow);
    let submission = view.proposals[0].milestones[0]
        .submissions
        .last()
        .ok_or("submission")?;
    assert!(submission.deliverable.is_none());
    success(
        &provider,
        &client,
        reject(project_id, milestone_id, submission.submission_id)?,
    )
    .await?;
    assert!(
        project(&provider, project_id)
            .await?
            .active_dispute_id
            .is_none()
    );
    Ok(())
}
