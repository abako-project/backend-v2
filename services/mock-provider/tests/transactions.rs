//! The same contract suite exercises both storage implementations.
#![cfg(feature = "mock-seed")]

use std::{
    error::Error,
    sync::atomic::{AtomicU64, Ordering},
};

use generated_contracts::*;
use mock_provider::Provider;
use subxt_signer::sr25519::Keypair;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const NOW: UnixSeconds = UnixSeconds::new(1_788_912_000);
static OPERATIONS: AtomicU64 = AtomicU64::new(1);

fn key(seed: u8) -> TestResult<Keypair> {
    Ok(Keypair::from_secret_key([seed; 32])?)
}
fn account(key: &Keypair) -> AccountId32 {
    AccountId32::from_bytes(key.public_key().0)
}
fn window() -> TestResult<WeekWindow> {
    let week = Week::new(2026, 40)?;
    Ok(WeekWindow::new(week, week)?)
}
fn capacity(minutes: u32) -> CalendarDefinition {
    CalendarDefinition {
        default_weekly_minutes: Minutes::new(minutes),
        overrides: Vec::new(),
    }
}

async fn signed(
    provider: &Provider,
    key: &Keypair,
    command: ProviderCommand,
) -> TestResult<SignedContractCallV1> {
    let mut id = [0; 16];
    id[..8].copy_from_slice(&OPERATIONS.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let call = UnsignedContractCallV1::new(
        provider.info().await?.provider_instance_id,
        OperationId::from_bytes(id),
        account(key),
        provider.nonce(account(key)).await?.nonce,
        UnixSeconds::new(NOW.get() + 300),
        command,
    )?;
    let signature = Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
    Ok(SignedContractCallV1 { call, signature })
}
async fn send(
    provider: &Provider,
    key: &Keypair,
    command: ProviderCommand,
) -> TestResult<OperationReceipt> {
    Ok(provider
        .execute(signed(provider, key, command).await?, NOW)
        .await?)
}
async fn success(
    provider: &Provider,
    key: &Keypair,
    command: ProviderCommand,
) -> TestResult<OperationReceipt> {
    let receipt = send(provider, key, command).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    Ok(receipt)
}
async fn project(provider: &Provider, id: EntityId) -> TestResult<ProjectView> {
    provider
        .snapshot()
        .await?
        .projects
        .into_iter()
        .find(|project| project.project_id == id)
        .ok_or_else(|| "missing project".into())
}
fn proposal() -> TestResult<ProposalDefinition> {
    Ok(ProposalDefinition {
        title: "Accepted plan".into(),
        description: String::new(),
        milestones: vec![MilestoneDefinition {
            key: 1,
            title: "Deliver".into(),
            window: window()?,
            coordinator_fee: Money::new(20),
            coordinator_minutes: Minutes::new(10),
            requirements: vec![RequirementDefinition {
                key: 1,
                role_id: 2,
                skill_ids: vec![1, 2, 3],
                minutes: Minutes::new(60),
                budget: Money::new(100),
            }],
        }],
    })
}
async fn register(
    provider: &Provider,
    key: &Keypair,
    skills: Vec<u32>,
    minutes: u32,
) -> TestResult {
    success(
        provider,
        key,
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Worker".into(),
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: skills,
            },
            calendar: capacity(minutes),
        }),
    )
    .await?;
    Ok(())
}
async fn setup(provider: &Provider) -> TestResult<(Keypair, Keypair, Keypair, Keypair)> {
    let catalog = provider.snapshot().await?.catalog;
    assert_eq!(catalog.roles.len(), 9);
    assert_eq!(catalog.skills.len(), 33);
    assert_eq!(catalog.skills[15].name, "graphql");
    let root = key(1)?;
    let coordinator = key(2)?;
    let worker = key(3)?;
    let client = key(4)?;
    register(provider, &coordinator, vec![1, 2, 3], 2400).await?;
    register(provider, &worker, vec![1, 2, 3], 60).await?;
    success(
        provider,
        &root,
        ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
            account: account(&coordinator),
        }),
    )
    .await?;
    success(
        provider,
        &coordinator,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Coordinator,
        }),
    )
    .await?;
    success(
        provider,
        &root,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: account(&client),
            amount: Money::new(1000),
        }),
    )
    .await?;
    Ok((root, coordinator, worker, client))
}

async fn delivered_plan(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
    definition: ProposalDefinition,
) -> TestResult<(EntityId, EntityId)> {
    let id = success(
        provider,
        client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Work request".into(),
            description: String::new(),
        }),
    )
    .await?
    .created_entity_id
    .ok_or("missing project ID")?;
    let quote = PlanningQuote {
        fee: Money::new(10),
        minutes: Minutes::new(30),
        window: window()?,
    };
    success(
        provider,
        coordinator,
        ProviderCommand::QuotePlanning {
            project_id: id,
            quote,
        },
    )
    .await?;
    let view = project(provider, id).await?;
    success(
        provider,
        client,
        ProviderCommand::AcceptPlanningQuote {
            project_id: id,
            expected_revision: view.planning.revision,
        },
    )
    .await?;
    let proposal_id = success(
        provider,
        coordinator,
        ProviderCommand::CreateProposal {
            project_id: id,
            proposal: definition,
        },
    )
    .await?
    .created_entity_id
    .ok_or("missing proposal ID")?;
    let drafted = project(provider, id).await?;
    assert!(drafted.proposals[0].milestones[0].status.is_none());
    assert!(
        drafted.proposals[0].milestones[0]
            .task_storage
            .tasks
            .is_empty()
    );
    success(
        provider,
        coordinator,
        ProviderCommand::SubmitProposal {
            project_id: id,
            proposal_id,
        },
    )
    .await?;
    let view = project(provider, id).await?;
    success(
        provider,
        client,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: id,
            expected_revision: view.planning.revision,
        },
    )
    .await?;
    let view = project(provider, id).await?;
    assert_eq!(view.planning.escrow, Money::ZERO);
    assert_eq!(view.execution_escrow, Money::ZERO);
    assert_eq!(view.proposals[0].status, ProposalStatus::PendingApproval);
    Ok((id, proposal_id))
}
async fn approve(
    provider: &Provider,
    client: &Keypair,
    id: EntityId,
    proposal_id: EntityId,
) -> TestResult<OperationReceipt> {
    let view = project(provider, id).await?;
    send(
        provider,
        client,
        ProviderCommand::ApproveExecution {
            project_id: id,
            proposal_id,
            expected_revision: view.proposals[0].revision,
        },
    )
    .await
}

async fn lifecycle(provider: Provider) -> TestResult {
    let (_root, coordinator, worker, client) = setup(&provider).await?;
    let intruder = key(5)?;
    register(&provider, &intruder, vec![1, 2], 2400).await?;
    let (id, proposal_id) = delivered_plan(&provider, &coordinator, &client, proposal()?).await?;
    assert_eq!(
        approve(&provider, &client, id, proposal_id).await?.outcome,
        ExecutionOutcome::Success
    );
    let (milestone_id, storage_id) =
        assert_assignment_and_calendar_guard(&provider, &worker, id).await?;
    exercise_task_permissions(
        &provider,
        &coordinator,
        &worker,
        &client,
        &intruder,
        id,
        storage_id,
    )
    .await?;
    complete_milestone_and_assert_settlement(
        &provider,
        &coordinator,
        &worker,
        &client,
        id,
        milestone_id,
    )
    .await
}

async fn assert_assignment_and_calendar_guard(
    provider: &Provider,
    worker: &Keypair,
    project_id: EntityId,
) -> TestResult<(EntityId, EntityId)> {
    let view = project(provider, project_id).await?;
    let milestone = &view.proposals[0].milestones[0];
    assert_eq!(milestone.assignments[0].worker, account(worker));
    let milestone_id = milestone.milestone_id;
    let storage_id = milestone.task_storage.task_storage_id;
    let reserved = provider.snapshot().await?.workers;
    let failed = send(provider, worker, ProviderCommand::SetCalendar(capacity(59))).await?;
    assert_eq!(
        failed.outcome,
        ExecutionOutcome::Failed("calendar_overcommitted".into())
    );
    assert_eq!(provider.snapshot().await?.workers, reserved);
    Ok((milestone_id, storage_id))
}

async fn exercise_task_permissions(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
    intruder: &Keypair,
    project_id: EntityId,
    storage_id: EntityId,
) -> TestResult {
    let reserved = provider.snapshot().await?.workers;
    let task = TaskDefinition {
        title: "Build".into(),
        description: String::new(),
        task_type: TaskType::Feature,
        priority: TaskPriority::High,
        status: TaskStatus::ToDo,
        assignees: vec![account(worker)],
        estimated_minutes: Minutes::new(60),
        logged_minutes: Minutes::ZERO,
        due_at: None,
    };
    let client_task = send(
        provider,
        client,
        ProviderCommand::CreateTask {
            project_id,
            task_storage_id: storage_id,
            task: task.clone(),
        },
    )
    .await?;
    assert_eq!(
        client_task.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    success(
        provider,
        coordinator,
        ProviderCommand::CreateTask {
            project_id,
            task_storage_id: storage_id,
            task: task.clone(),
        },
    )
    .await?;
    let task_view = project(provider, project_id).await?.proposals[0].milestones[0]
        .task_storage
        .tasks[0]
        .clone();
    assert_eq!(task_view.reporter, account(coordinator));
    assert_eq!(task_view.created_at, NOW);
    success(
        provider,
        worker,
        ProviderCommand::UpdateTaskProgress {
            project_id,
            task_storage_id: storage_id,
            task_id: task_view.task_id,
            progress: TaskProgressRequest {
                status: TaskStatus::Done,
                logged_minutes: Minutes::new(10_000),
            },
        },
    )
    .await?;
    let unauthorized_edit = send(
        provider,
        worker,
        ProviderCommand::EditTask {
            project_id,
            task_storage_id: storage_id,
            task_id: task_view.task_id,
            task: task.clone(),
        },
    )
    .await?;
    assert_eq!(
        unauthorized_edit.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    let mut changed = task;
    changed.assignees = vec![account(intruder)];
    success(
        provider,
        coordinator,
        ProviderCommand::EditTask {
            project_id,
            task_storage_id: storage_id,
            task_id: task_view.task_id,
            task: changed,
        },
    )
    .await?;
    assert_eq!(provider.snapshot().await?.workers, reserved);
    Ok(())
}

async fn complete_milestone_and_assert_settlement(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
    project_id: EntityId,
    milestone_id: EntityId,
) -> TestResult {
    let requested = RequestMilestoneCompletionRequest {
        worker_ratings: vec![WorkerRating {
            worker: account(worker),
            score: Score::new(9)?,
        }],
    };
    success(
        provider,
        coordinator,
        ProviderCommand::RequestMilestoneCompletion {
            project_id,
            milestone_id,
            request: requested,
        },
    )
    .await?;
    let command = ProviderCommand::AcceptMilestoneCompletion {
        project_id,
        milestone_id,
        request: AcceptMilestoneCompletionRequest {
            coordinator_score: Score::new(8)?,
            team_rating: TeamRating::Client(Score::new(7)?),
        },
    };
    let signed = signed(provider, client, command.clone()).await?;
    let receipt = provider.execute(signed.clone(), NOW).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    let snapshot = provider.snapshot().await?;
    assert_settlement_snapshot(&snapshot, coordinator, worker, client)?;
    assert_eq!(
        project(provider, project_id).await?.execution_escrow,
        Money::ZERO
    );
    let replay = provider
        .execute(signed.clone(), UnixSeconds::new(NOW.get() + 1000))
        .await?;
    assert_eq!(receipt, replay);
    assert_eq!(snapshot, provider.snapshot().await?);
    assert_eq!(
        send(provider, client, command).await?.outcome,
        ExecutionOutcome::Failed("invalid_milestone_state".into())
    );
    assert_eq!(snapshot, provider.snapshot().await?);
    let mut forged = signed;
    forged.signature = Sr25519Signature::from_bytes([0; 64]);
    assert_eq!(
        provider
            .execute(forged, NOW)
            .await
            .err()
            .ok_or("expected signature rejection")?
            .code(),
        "invalid_signature"
    );
    Ok(())
}

fn assert_settlement_snapshot(
    snapshot: &ProviderSnapshot,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
) -> TestResult {
    let worker_view = snapshot
        .workers
        .iter()
        .find(|item| item.account == account(worker))
        .ok_or("missing worker")?;
    assert_eq!(
        worker_view.worker_score,
        ReputationView {
            weighted_score_sum: 48_000,
            rated_minutes: 60
        }
    );
    assert_eq!(worker_view.calendar.reservations.len(), 1);
    let coordinator_view = snapshot
        .workers
        .iter()
        .find(|item| item.account == account(coordinator))
        .ok_or("missing coordinator")?;
    assert_eq!(
        coordinator_view.coordinator_score,
        ReputationView {
            weighted_score_sum: 8000,
            rated_minutes: 10
        }
    );
    assert_eq!(coordinator_view.worker_score.rated_minutes, 0);
    assert_eq!(
        snapshot
            .balances
            .iter()
            .find(|balance| balance.account == account(client))
            .ok_or("client balance")?
            .available,
        Money::new(870)
    );
    assert_eq!(
        snapshot
            .balances
            .iter()
            .find(|balance| balance.account == account(coordinator))
            .ok_or("coordinator balance")?
            .available,
        Money::new(30)
    );
    assert_eq!(
        snapshot
            .balances
            .iter()
            .find(|balance| balance.account == account(worker))
            .ok_or("worker balance")?
            .available,
        Money::new(100)
    );
    Ok(())
}

async fn races_and_rollback(provider: Provider) -> TestResult {
    let (root, coordinator, worker, client) = setup(&provider).await?;
    let second = key(6)?;
    success(
        &provider,
        &root,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: account(&second),
            amount: Money::new(1000),
        }),
    )
    .await?;
    let mut impossible = proposal()?;
    let mut missing = impossible.milestones[0].requirements[0].clone();
    missing.key = 2;
    missing.skill_ids = vec![33];
    impossible.milestones[0].requirements.push(missing);
    let (bad_id, bad_proposal) =
        delivered_plan(&provider, &coordinator, &client, impossible).await?;
    let before = provider.snapshot().await?;
    assert_eq!(
        approve(&provider, &client, bad_id, bad_proposal)
            .await?
            .outcome,
        ExecutionOutcome::Failed("no_available_candidate".into())
    );
    assert_eq!(before, provider.snapshot().await?);
    let (first_id, first_proposal) =
        delivered_plan(&provider, &coordinator, &client, proposal()?).await?;
    let (second_id, second_proposal) =
        delivered_plan(&provider, &coordinator, &second, proposal()?).await?;
    let first_call = signed(
        &provider,
        &client,
        ProviderCommand::ApproveExecution {
            project_id: first_id,
            proposal_id: first_proposal,
            expected_revision: 1,
        },
    )
    .await?;
    let second_call = signed(
        &provider,
        &second,
        ProviderCommand::ApproveExecution {
            project_id: second_id,
            proposal_id: second_proposal,
            expected_revision: 1,
        },
    )
    .await?;
    let (first_result, second_result) = tokio::join!(
        provider.execute(first_call, NOW),
        provider.execute(second_call, NOW)
    );
    let outcomes = [first_result?.outcome, second_result?.outcome];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ExecutionOutcome::Success)
            .count(),
        1
    );
    let snapshot = provider.snapshot().await?;
    let worker = snapshot
        .workers
        .iter()
        .find(|item| item.account == account(&worker))
        .ok_or("missing worker")?;
    assert_eq!(
        worker
            .calendar
            .reservations
            .iter()
            .map(|item| item.minutes.get())
            .sum::<u32>(),
        60
    );
    assert_eq!(
        snapshot
            .projects
            .iter()
            .map(|item| item.execution_escrow.units())
            .sum::<u64>(),
        120
    );
    assert_eq!(
        snapshot
            .projects
            .iter()
            .flat_map(|item| &item.proposals)
            .filter(|item| item.status == ProposalStatus::Approved)
            .count(),
        1
    );
    Ok(())
}

async fn security_and_revisions(provider: Provider) -> TestResult {
    let (root, coordinator, worker, client) = setup(&provider).await?;
    assert_planning_revisions(&provider, &coordinator, &client).await?;
    assert_coordinator_permissions(&provider, &root, &worker).await?;
    assert_execution_revision(&provider, &coordinator, &client).await?;
    assert_signed_call_integrity(&provider, &worker).await
}

async fn assert_planning_revisions(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
) -> TestResult {
    let quoted_id = success(
        provider,
        client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Reviewed quote".into(),
            description: String::new(),
        }),
    )
    .await?
    .created_entity_id
    .ok_or("project ID")?;
    for fee in [10, 20] {
        success(
            provider,
            coordinator,
            ProviderCommand::QuotePlanning {
                project_id: quoted_id,
                quote: PlanningQuote {
                    fee: Money::new(fee),
                    minutes: Minutes::new(30),
                    window: window()?,
                },
            },
        )
        .await?;
    }
    let before_quote = provider.snapshot().await?;
    assert_eq!(
        send(
            provider,
            client,
            ProviderCommand::AcceptPlanningQuote {
                project_id: quoted_id,
                expected_revision: 1
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("revision_conflict".into())
    );
    assert_eq!(provider.snapshot().await?, before_quote);
    success(
        provider,
        client,
        ProviderCommand::AcceptPlanningQuote {
            project_id: quoted_id,
            expected_revision: 2,
        },
    )
    .await?;
    assert_delivery_revision(provider, coordinator, client, quoted_id).await
}

async fn assert_delivery_revision(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
    quoted_id: EntityId,
) -> TestResult {
    let quoted_proposal = success(
        provider,
        coordinator,
        ProviderCommand::CreateProposal {
            project_id: quoted_id,
            proposal: proposal()?,
        },
    )
    .await?
    .created_entity_id
    .ok_or("proposal ID")?;
    success(
        provider,
        coordinator,
        ProviderCommand::SubmitProposal {
            project_id: quoted_id,
            proposal_id: quoted_proposal,
        },
    )
    .await?;
    let delivered_revision = project(provider, quoted_id).await?.planning.revision;
    success(
        provider,
        client,
        ProviderCommand::RequestProposalChanges {
            project_id: quoted_id,
            proposal_id: quoted_proposal,
            request: RequestChangesRequest {
                reference: "Revise the deliverable".into(),
            },
        },
    )
    .await?;
    let before_delivery = provider.snapshot().await?;
    assert_eq!(
        send(
            provider,
            client,
            ProviderCommand::AcceptPlanningDelivery {
                project_id: quoted_id,
                expected_revision: delivered_revision
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("revision_conflict".into())
    );
    assert_eq!(provider.snapshot().await?, before_delivery);
    Ok(())
}

async fn assert_coordinator_permissions(
    provider: &Provider,
    root: &Keypair,
    worker: &Keypair,
) -> TestResult {
    let attempt = send(
        provider,
        worker,
        ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
            account: account(worker),
        }),
    )
    .await?;
    assert_eq!(
        attempt.outcome,
        ExecutionOutcome::Failed("system_origin_required".into())
    );
    assert_eq!(
        send(
            provider,
            worker,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Coordinator
            })
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("coordinator_not_eligible".into())
    );
    for command in [
        ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
            kind: CatalogKind::Role,
            id: 1,
            name: "renamed".into(),
        }),
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Role,
            id: 1,
        }),
    ] {
        assert_eq!(
            send(provider, root, command).await?.outcome,
            ExecutionOutcome::Failed("fixed_coordinator_role".into())
        );
    }
    Ok(())
}

async fn assert_execution_revision(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
) -> TestResult {
    let (id, proposal_id) = delivered_plan(provider, coordinator, client, proposal()?).await?;
    let before = provider.snapshot().await?;
    assert_eq!(
        send(
            provider,
            client,
            ProviderCommand::ApproveExecution {
                project_id: id,
                proposal_id,
                expected_revision: 0
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("revision_conflict".into())
    );
    assert_eq!(before, provider.snapshot().await?);
    Ok(())
}

async fn assert_signed_call_integrity(provider: &Provider, worker: &Keypair) -> TestResult {
    let command = ProviderCommand::SetCalendar(capacity(80));
    let valid = signed(provider, worker, command.clone()).await?;
    let mut invalid = valid.clone();
    invalid.call.provider_instance_id = ProviderInstanceId::from_bytes([99; 16]);
    invalid.signature =
        Sr25519Signature::from_bytes(worker.sign(&invalid.call.signable_bytes()?).0);
    assert_eq!(
        provider
            .execute(invalid, NOW)
            .await
            .err()
            .ok_or("expected instance rejection")?
            .code(),
        "provider_instance_mismatch"
    );
    assert_eq!(
        provider
            .execute(valid.clone(), UnixSeconds::new(NOW.get() + 301))
            .await
            .err()
            .ok_or("expected expiry rejection")?
            .code(),
        "expired_call"
    );
    let mut manipulated = valid.clone();
    manipulated.call.command = ProviderCommand::SetCalendar(capacity(81));
    assert_eq!(
        provider
            .execute(manipulated, NOW)
            .await
            .err()
            .ok_or("expected signature rejection")?
            .code(),
        "invalid_signature"
    );
    let original = provider.execute(valid.clone(), NOW).await?;
    assert_eq!(original.outcome, ExecutionOutcome::Success);
    let mut collision = valid.clone();
    collision.call.command = ProviderCommand::SetCalendar(capacity(82));
    collision.signature =
        Sr25519Signature::from_bytes(worker.sign(&collision.call.signable_bytes()?).0);
    assert_eq!(
        provider
            .execute(collision, NOW)
            .await
            .err()
            .ok_or("expected ID rejection")?
            .code(),
        "idempotency_conflict"
    );
    let mut forged_replay = valid.clone();
    forged_replay.signature = Sr25519Signature::from_bytes([0; 64]);
    assert_eq!(
        provider
            .execute(forged_replay, NOW)
            .await
            .err()
            .ok_or("expected signature rejection")?
            .code(),
        "invalid_signature"
    );
    assert_eq!(
        provider
            .execute(valid, UnixSeconds::new(NOW.get() + 1000))
            .await?,
        original
    );
    Ok(())
}

async fn delegated_ratings(provider: Provider) -> TestResult {
    let (root, coordinator, worker, client) = setup(&provider).await?;
    success(
        &provider,
        &root,
        ProviderCommand::SetScorePolicy(ScorePolicy::new(
            Percentage::new(0)?,
            Percentage::new(100)?,
        )?),
    )
    .await?;
    let (id, proposal_id) = delivered_plan(&provider, &coordinator, &client, proposal()?).await?;
    assert_eq!(
        approve(&provider, &client, id, proposal_id).await?.outcome,
        ExecutionOutcome::Success
    );
    let milestone_id = project(&provider, id).await?.proposals[0].milestones[0].milestone_id;
    success(
        &provider,
        &coordinator,
        ProviderCommand::RequestMilestoneCompletion {
            project_id: id,
            milestone_id,
            request: RequestMilestoneCompletionRequest {
                worker_ratings: vec![WorkerRating {
                    worker: account(&worker),
                    score: Score::new(9)?,
                }],
            },
        },
    )
    .await?;
    success(
        &provider,
        &client,
        ProviderCommand::AcceptMilestoneCompletion {
            project_id: id,
            milestone_id,
            request: AcceptMilestoneCompletionRequest {
                coordinator_score: Score::new(6)?,
                team_rating: TeamRating::DelegateToCoordinator,
            },
        },
    )
    .await?;
    let snapshot = provider.snapshot().await?;
    let worker = snapshot
        .workers
        .iter()
        .find(|item| item.account == account(&worker))
        .ok_or("missing worker")?;
    assert_eq!(
        worker.worker_score,
        ReputationView {
            weighted_score_sum: 54_000,
            rated_minutes: 60
        }
    );
    Ok(())
}

async fn freezing_permissions(provider: Provider) -> TestResult {
    let (_root, coordinator, worker, client) = setup(&provider).await?;
    success(
        &provider,
        &worker,
        ProviderCommand::SetCalendar(capacity(600)),
    )
    .await?;
    let reason = ReasonRequest {
        reason: "Needs agreed resolution".into(),
    };
    for actor in [&client, &coordinator] {
        assert_planning_dispute_freezes(&provider, &coordinator, &worker, &client, actor, &reason)
            .await?;
        assert_milestone_dispute_and_cancellation_freeze(
            &provider,
            &coordinator,
            &worker,
            &client,
            actor,
            &reason,
        )
        .await?;
    }
    Ok(())
}

async fn assert_planning_dispute_freezes(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
    actor: &Keypair,
    reason: &ReasonRequest,
) -> TestResult {
    let planning_id = success(
        provider,
        client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Plan".into(),
            description: String::new(),
        }),
    )
    .await?
    .created_entity_id
    .ok_or("project ID")?;
    success(
        provider,
        coordinator,
        ProviderCommand::QuotePlanning {
            project_id: planning_id,
            quote: PlanningQuote {
                fee: Money::new(10),
                minutes: Minutes::new(30),
                window: window()?,
            },
        },
    )
    .await?;
    success(
        provider,
        client,
        ProviderCommand::AcceptPlanningQuote {
            project_id: planning_id,
            expected_revision: 1,
        },
    )
    .await?;
    let before = provider.snapshot().await?;
    let dispute = ProviderCommand::DisputePlanning {
        project_id: planning_id,
        request: reason.clone(),
    };
    assert_eq!(
        send(provider, worker, dispute.clone()).await?.outcome,
        ExecutionOutcome::Failed("project_party_required".into())
    );
    assert_eq!(provider.snapshot().await?, before);
    success(provider, actor, dispute).await?;
    let view = project(provider, planning_id).await?;
    assert_eq!(view.planning.status, PlanningStatus::Disputed);
    assert_eq!(view.planning.escrow, Money::new(10));
    assert!(view.planning.frozen);
    assert_eq!(provider.snapshot().await?.balances, before.balances);
    assert_eq!(provider.snapshot().await?.workers, before.workers);
    assert_eq!(
        send(
            provider,
            client,
            ProviderCommand::AcceptPlanningDelivery {
                project_id: planning_id,
                expected_revision: view.planning.revision
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("invalid_planning_state".into())
    );
    Ok(())
}

async fn assert_milestone_dispute_and_cancellation_freeze(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
    actor: &Keypair,
    reason: &ReasonRequest,
) -> TestResult {
    let (project_id, proposal_id) =
        delivered_plan(provider, coordinator, client, proposal()?).await?;
    assert_eq!(
        approve(provider, client, project_id, proposal_id)
            .await?
            .outcome,
        ExecutionOutcome::Success
    );
    let milestone_id = project(provider, project_id).await?.proposals[0].milestones[0].milestone_id;
    success(
        provider,
        coordinator,
        ProviderCommand::RequestMilestoneCompletion {
            project_id,
            milestone_id,
            request: RequestMilestoneCompletionRequest {
                worker_ratings: vec![WorkerRating {
                    worker: account(worker),
                    score: Score::new(8)?,
                }],
            },
        },
    )
    .await?;
    let before = provider.snapshot().await?;
    let dispute = ProviderCommand::DisputeMilestone {
        project_id,
        milestone_id,
        request: reason.clone(),
    };
    assert_eq!(
        send(provider, worker, dispute.clone()).await?.outcome,
        ExecutionOutcome::Failed("project_party_required".into())
    );
    assert_eq!(provider.snapshot().await?, before);
    success(provider, actor, dispute).await?;
    let view = project(provider, project_id).await?;
    assert!(view.proposals[0].milestones[0].frozen);
    assert_eq!(
        view.proposals[0].milestones[0].status,
        Some(MilestoneStatus::Disputed)
    );
    assert_eq!(view.execution_escrow, Money::new(120));
    assert_eq!(
        send(
            provider,
            client,
            ProviderCommand::AcceptMilestoneCompletion {
                project_id,
                milestone_id,
                request: AcceptMilestoneCompletionRequest {
                    coordinator_score: Score::new(7)?,
                    team_rating: TeamRating::DelegateToCoordinator
                }
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("invalid_milestone_state".into())
    );
    assert_cancellation_freezes(provider, worker, actor, project_id, reason, &before).await
}

async fn assert_cancellation_freezes(
    provider: &Provider,
    worker: &Keypair,
    actor: &Keypair,
    project_id: EntityId,
    reason: &ReasonRequest,
    before: &ProviderSnapshot,
) -> TestResult {
    let cancel = ProviderCommand::CancelProject {
        project_id,
        request: reason.clone(),
    };
    assert_eq!(
        send(provider, worker, cancel.clone()).await?.outcome,
        ExecutionOutcome::Failed("project_party_required".into())
    );
    success(provider, actor, cancel).await?;
    let cancelled = project(provider, project_id).await?;
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.execution_escrow, Money::new(120));
    assert_eq!(cancelled.proposals[0].status, ProposalStatus::Cancelled);
    assert_eq!(provider.snapshot().await?.balances, before.balances);
    assert_eq!(provider.snapshot().await?.workers, before.workers);
    Ok(())
}

macro_rules! suite {
    ($feature:literal, $module:ident, $factory:expr) => {
        #[cfg(feature = $feature)]
        mod $module {
            use super::*;
            async fn provider() -> TestResult<Provider> {
                $factory.await
            }
            #[tokio::test]
            async fn full_lifecycle() -> TestResult {
                lifecycle(provider().await?).await
            }
            #[tokio::test]
            async fn atomic_competing_approvals() -> TestResult {
                races_and_rollback(provider().await?).await
            }
            #[tokio::test]
            async fn authorization_replay_and_revisions() -> TestResult {
                security_and_revisions(provider().await?).await
            }
            #[tokio::test]
            async fn delegation_uses_individual_rating_without_fabricated_vote() -> TestResult {
                delegated_ratings(provider().await?).await
            }
            #[tokio::test]
            async fn cancellation_and_disputes_require_either_project_party_and_freeze_funds()
            -> TestResult {
                freezing_permissions(provider().await?).await
            }
        }
    };
}
suite!("storage-memory", memory, async {
    Ok(Provider::memory(account(&key(1)?))?)
});
suite!("storage-sqlite", sqlite, async {
    Ok(Provider::sqlite("sqlite::memory:", account(&key(1)?)).await?)
});

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn independent_runtimes_share_transactions_and_retained_generation() -> TestResult {
    let root = key(1)?;
    let id = OPERATIONS.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "kunveno-mock-test-{}-{id}.sqlite",
        std::process::id()
    ));
    assert!(!path.exists());
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let first = Provider::sqlite(&url, account(&root)).await?;
    let generation = first.info().await?;
    let second = Provider::sqlite(&url, account(&root)).await?;
    assert_eq!(generation, second.info().await?);
    let rename = ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
        kind: CatalogKind::Skill,
        id: 1,
        name: "Rust edited".into(),
    });
    success(&first, &root, rename).await?;
    let command = ProviderCommand::FundAccount(FundAccountRequest {
        account: account(&key(9)?),
        amount: Money::new(5),
    });
    let call = signed(&first, &root, command).await?;
    let (left, right) = tokio::join!(first.execute(call.clone(), NOW), second.execute(call, NOW));
    assert_eq!(left?, right?);
    assert_eq!(first.snapshot().await?, second.snapshot().await?);
    assert_eq!(first.snapshot().await?.balances[0].available, Money::new(5));
    drop(first);
    drop(second);
    let restored = Provider::sqlite(&url, account(&root)).await?;
    assert_eq!(restored.info().await?, generation);
    assert_eq!(
        restored.snapshot().await?.catalog.skills[0].name,
        "Rust edited"
    );
    assert!(Provider::sqlite(&url, account(&key(2)?)).await.is_err());
    drop(restored);
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn signed_http_auth_receipts_and_event_replay() -> TestResult {
    let root = key(1)?;
    let provider = Provider::sqlite("sqlite::memory:", account(&root)).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let token = "local-test-service-token-32-bytes-minimum";
    let app = mock_provider::router(provider.clone(), token)?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    assert_http_boundaries(&client, &base, token).await?;
    let mut call = signed(
        &provider,
        &root,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: account(&key(8)?),
            amount: Money::new(50),
        }),
    )
    .await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    call.call.expires_at = UnixSeconds::new(now + 300);
    call.signature = Sr25519Signature::from_bytes(root.sign(&call.call.signable_bytes()?).0);
    // Dropping the successful response models a lost application reply. Recovery
    // uses the same receipt/bytes, not a new business operation.
    let response = client
        .post(format!("{base}/internal/contracts/call"))
        .bearer_auth(token)
        .json(&call)
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    drop(response);
    assert_http_receipt_and_events(&client, &provider, &base, token, &call).await?;
    server.abort();
    assert!(server.await.is_err_and(|error| error.is_cancelled()));
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
async fn assert_http_boundaries(client: &reqwest::Client, base: &str, token: &str) -> TestResult {
    assert_eq!(
        client.get(format!("{base}/health")).send().await?.status(),
        reqwest::StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{base}/internal/snapshot"))
            .send()
            .await?
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let response = client
        .post(format!("{base}/internal/contracts/call"))
        .bearer_auth(token)
        .json(&serde_json::json!({"unknown": "field"}))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json::<ApiError>().await?.code,
        "invalid_contract_json"
    );
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
async fn assert_http_receipt_and_events(
    client: &reqwest::Client,
    provider: &Provider,
    base: &str,
    token: &str,
    call: &SignedContractCallV1,
) -> TestResult {
    let receipt = client
        .get(format!(
            "{base}/internal/receipts/{}",
            call.call.operation_id
        ))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<OperationReceipt>()
        .await?;
    let replay = client
        .post(format!("{base}/internal/contracts/call"))
        .bearer_auth(token)
        .json(&call)
        .send()
        .await?
        .error_for_status()?
        .json::<OperationReceipt>()
        .await?;
    assert_eq!(receipt, replay);
    let events = client
        .get(format!("{base}/internal/events?after=0&limit=100"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<ProviderEvents>()
        .await?;
    assert_eq!(events.events.len(), 1);
    let repeated = client
        .get(format!("{base}/internal/events?after=0&limit=100"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<ProviderEvents>()
        .await?;
    assert_eq!(events, repeated);
    assert!(
        provider
            .events(events.next_cursor, 100)
            .await?
            .events
            .is_empty()
    );
    Ok(())
}
