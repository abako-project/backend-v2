use super::*;

async fn complete(
    provider: &Provider,
    coordinator: &Keypair,
    client: &Keypair,
    worker: &Keypair,
    project_id: EntityId,
    milestone_id: EntityId,
) -> TestResult {
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
                    score: Score::new(10)?,
                }],
            },
        },
    )
    .await?;
    success(
        provider,
        client,
        ProviderCommand::AcceptMilestoneCompletion {
            project_id,
            milestone_id,
            request: AcceptMilestoneCompletionRequest {
                submission_id: receipt.created_entity_id.ok_or("missing submission")?,
                coordinator_score: Score::new(8)?,
                team_rating: TeamRating::DelegateToCoordinator,
            },
        },
    )
    .await?;
    Ok(())
}

async fn empty_draft(
    provider: &Provider,
) -> TestResult<(Keypair, Keypair, Keypair, EntityId, EntityId)> {
    let (_root, coordinator, worker, client) = setup(provider).await?;
    let project_id = success(
        provider,
        &client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Plan with empty task storages".into(),
            description: String::new(),
        }),
    )
    .await?
    .created_entity_id
    .ok_or("project ID")?;
    success(
        provider,
        &coordinator,
        ProviderCommand::QuotePlanning {
            project_id,
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
        &client,
        ProviderCommand::AcceptPlanningQuote {
            project_id,
            expected_revision: 1,
        },
    )
    .await?;
    let mut definition = proposal()?;
    let mut second = definition.milestones[0].clone();
    second.key = 2;
    definition.milestones.push(second);
    let proposal_id = success(
        provider,
        &coordinator,
        ProviderCommand::CreateProposal {
            project_id,
            proposal: definition,
        },
    )
    .await?
    .created_entity_id
    .ok_or("proposal ID")?;
    Ok((coordinator, worker, client, project_id, proposal_id))
}

pub(super) async fn empty_task_storages(provider: Provider) -> TestResult {
    let (coordinator, worker, client, project_id, proposal_id) = empty_draft(&provider).await?;
    success(
        &provider,
        &worker,
        ProviderCommand::SetCalendar(capacity(120)),
    )
    .await?;
    let before = project(&provider, project_id).await?;
    let storages: Vec<_> = before.proposals[0]
        .milestones
        .iter()
        .map(|milestone| milestone.task_storage.task_storage_id)
        .collect();
    assert_eq!(storages.len(), 2);
    assert_ne!(storages[0], storages[1]);
    assert!(
        before.proposals[0]
            .milestones
            .iter()
            .all(|m| m.task_storage.tasks.is_empty())
    );
    success(
        &provider,
        &coordinator,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
    )
    .await?;
    let delivered = project(&provider, project_id).await?;
    assert_eq!(
        delivered.proposals[0].status,
        ProposalStatus::PendingApproval
    );
    assert_eq!(delivered.execution_escrow, Money::ZERO);
    assert_eq!(
        approve(&provider, &client, project_id, proposal_id)
            .await?
            .outcome,
        ExecutionOutcome::Failed("planning_not_accepted".into())
    );
    success(
        &provider,
        &client,
        ProviderCommand::AcceptPlanningDelivery {
            project_id,
            expected_revision: delivered.planning.revision,
        },
    )
    .await?;
    assert_eq!(
        approve(&provider, &client, project_id, proposal_id)
            .await?
            .outcome,
        ExecutionOutcome::Success
    );
    let approved = project(&provider, project_id).await?;
    assert_eq!(approved.execution_escrow, Money::new(240));
    for milestone in &approved.proposals[0].milestones {
        complete(
            &provider,
            &coordinator,
            &client,
            &worker,
            project_id,
            milestone.milestone_id,
        )
        .await?;
    }
    let completed = project(&provider, project_id).await?;
    assert!(completed.completed);
    assert_eq!(completed.execution_escrow, Money::ZERO);
    for (milestone, storage_id) in completed.proposals[0].milestones.iter().zip(storages) {
        assert_eq!(milestone.task_storage.task_storage_id, storage_id);
        assert!(milestone.task_storage.tasks.is_empty());
        assert_eq!(milestone.status, Some(MilestoneStatus::Completed));
    }
    Ok(())
}

async fn prepare_higher_scored_candidate(
    provider: &Provider,
    coordinator: &Keypair,
    worker: &Keypair,
    client: &Keypair,
) -> TestResult<Keypair> {
    success(
        provider,
        worker,
        ProviderCommand::SetCalendar(capacity(240)),
    )
    .await?;
    let high_score = key(6)?;
    register(provider, &high_score, vec![1, 2, 3, 4], 240).await?;

    let mut rating_plan = proposal()?;
    rating_plan.milestones[0].requirements[0].skill_ids = vec![4];
    let (rating_project, rating_proposal) =
        delivered_plan(provider, coordinator, client, rating_plan).await?;
    assert_eq!(
        approve(provider, client, rating_project, rating_proposal)
            .await?
            .outcome,
        ExecutionOutcome::Success
    );
    let rating_milestone =
        project(provider, rating_project).await?.proposals[0].milestones[0].milestone_id;
    complete(
        provider,
        coordinator,
        client,
        &high_score,
        rating_project,
        rating_milestone,
    )
    .await?;
    assert!(project(provider, rating_project).await?.completed);
    Ok(high_score)
}

pub(super) async fn sequential_and_continuity(provider: Provider) -> TestResult {
    let (_root, coordinator, worker, client) = setup(&provider).await?;
    let high_score =
        prepare_higher_scored_candidate(&provider, &coordinator, &worker, &client).await?;
    assert_score_advantage(&provider, &high_score, &worker).await?;

    success(
        &provider,
        &worker,
        ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: vec![1, 2, 3, 5],
            },
        }),
    )
    .await?;

    let mut definition = proposal()?;
    definition.milestones[0].requirements[0].skill_ids = vec![5];
    let mut second = definition.milestones[0].clone();
    second.key = 2;
    second.requirements[0].skill_ids = vec![1, 2, 3];
    definition.milestones.push(second);
    let (project_id, proposal_id) =
        delivered_plan(&provider, &coordinator, &client, definition).await?;
    assert_eq!(
        approve(&provider, &client, project_id, proposal_id)
            .await?
            .outcome,
        ExecutionOutcome::Success
    );
    let approved = project(&provider, project_id).await?;
    let milestones = &approved.proposals[0].milestones;
    assert_eq!(milestones[0].assignments[0].worker, account(&worker));
    assert_eq!(milestones[1].assignments[0].worker, account(&worker));
    assert_eq!(milestones[0].status, Some(MilestoneStatus::InProgress));
    assert_eq!(milestones[1].status, Some(MilestoneStatus::NotStarted));
    assert!(!approved.completed);
    assert_eq!(approved.execution_escrow, Money::new(240));
    let first_id = milestones[0].milestone_id;
    let second_id = milestones[1].milestone_id;
    assert_eq!(
        send(
            &provider,
            &coordinator,
            ProviderCommand::RequestMilestoneCompletion {
                project_id,
                milestone_id: second_id,
                request: RequestMilestoneCompletionRequest {
                    deliverable: evidence()?,
                    worker_ratings: vec![WorkerRating {
                        worker: account(&worker),
                        score: Score::new(8)?,
                    }],
                },
            },
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("invalid_milestone_state".into())
    );

    complete(
        &provider,
        &coordinator,
        &client,
        &worker,
        project_id,
        first_id,
    )
    .await?;
    let after_first = project(&provider, project_id).await?;
    assert_eq!(
        after_first.proposals[0].milestones[1].status,
        Some(MilestoneStatus::InProgress)
    );
    assert_eq!(after_first.execution_escrow, Money::new(120));
    assert!(!after_first.completed);
    complete(
        &provider,
        &coordinator,
        &client,
        &worker,
        project_id,
        second_id,
    )
    .await?;
    let finished = project(&provider, project_id).await?;
    assert!(finished.completed);
    assert_eq!(finished.execution_escrow, Money::ZERO);
    assert!(
        finished.proposals[0]
            .milestones
            .iter()
            .all(|milestone| milestone.status == Some(MilestoneStatus::Completed))
    );
    Ok(())
}

async fn assert_score_advantage(
    provider: &Provider,
    high_score: &Keypair,
    worker: &Keypair,
) -> TestResult {
    let scores = provider.snapshot().await?.workers;
    let high = scores
        .iter()
        .find(|candidate| candidate.account == account(high_score))
        .ok_or("high-scored worker missing")?;
    let prior = scores
        .iter()
        .find(|candidate| candidate.account == account(worker))
        .ok_or("prior worker missing")?;
    assert!(high.worker_score.rated_minutes > 0);
    assert_eq!(prior.worker_score.rated_minutes, 0);
    Ok(())
}
