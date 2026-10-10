use super::*;

pub(super) async fn unrated_delivery(provider: Provider) -> TestResult {
    let (_, coordinator, worker, client) = setup(&provider).await?;
    let (id, proposal_id) = delivered_plan(&provider, &coordinator, &client, proposal()?).await?;
    assert_eq!(
        approve(&provider, &client, id, proposal_id).await?.outcome,
        ExecutionOutcome::Success
    );
    let initial = provider.snapshot().await?;
    let view = project(&provider, id).await?;
    let milestone = &view.proposals[0].milestones[0];
    let command = ProviderCommand::SubmitMilestoneDelivery {
        project_id: id,
        milestone_id: milestone.milestone_id,
        request: SubmitMilestoneCompletionRequest { deliverable: None },
    };
    assert_eq!(
        send(&provider, &worker, command.clone()).await?.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    let submitted = success(&provider, &coordinator, command).await?;
    let submission_id = submitted.created_entity_id.ok_or("submission ID")?;
    let pending = project(&provider, id).await?;
    assert!(
        pending.proposals[0].milestones[0].submissions[0]
            .worker_ratings
            .is_empty()
    );
    assert!(
        pending.proposals[0].milestones[0].submissions[0]
            .deliverable
            .is_none()
    );
    assert_eq!(pending.execution_escrow, view.execution_escrow);
    assert_eq!(provider.snapshot().await?.balances, initial.balances);
    let accept = ProviderCommand::AcceptMilestoneDelivery {
        project_id: id,
        milestone_id: milestone.milestone_id,
        submission_id,
    };
    assert_eq!(
        send(&provider, &coordinator, accept.clone()).await?.outcome,
        ExecutionOutcome::Failed("client_required".into())
    );
    let stale = ProviderCommand::AcceptMilestoneDelivery {
        project_id: id,
        milestone_id: milestone.milestone_id,
        submission_id: EntityId::from_bytes([99; 16]),
    };
    assert_eq!(
        send(&provider, &client, stale).await?.outcome,
        ExecutionOutcome::Failed("submission_not_current".into())
    );
    let call = signed(&provider, &client, accept).await?;
    let receipt = provider.execute(call.clone(), NOW).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    let settled = provider.snapshot().await?;
    assert!(project(&provider, id).await?.completed);
    assert_eq!(project(&provider, id).await?.execution_escrow, Money::ZERO);
    for before in &initial.workers {
        let after = settled
            .workers
            .iter()
            .find(|w| w.account == before.account)
            .ok_or("worker")?;
        assert_eq!(after.worker_score, before.worker_score);
        assert_eq!(after.coordinator_score, before.coordinator_score);
    }
    for (account, fee) in [
        (account(&coordinator), milestone.definition.coordinator_fee),
        (
            account(&worker),
            milestone.definition.requirements[0].budget,
        ),
    ] {
        let before = initial
            .balances
            .iter()
            .find(|b| b.account == account)
            .map_or(Money::ZERO, |b| b.available);
        let after = settled
            .balances
            .iter()
            .find(|b| b.account == account)
            .ok_or("balance")?
            .available;
        assert_eq!(after, before.checked_add(fee)?);
    }
    assert_eq!(provider.execute(call, NOW).await?, receipt);
    assert_eq!(provider.snapshot().await?, settled);
    final_evaluations(&provider, id, milestone, &client, &coordinator, &worker).await?;
    Ok(())
}
async fn final_evaluations(
    provider: &Provider,
    id: EntityId,
    milestone: &MilestoneView,
    client: &Keypair,
    coordinator: &Keypair,
    worker: &Keypair,
) -> TestResult {
    use generated_contracts::{AccountRating, EvaluateProjectRequest};
    let before = provider.snapshot().await?;
    let vote = |ratings| ProviderCommand::EvaluateProject {
        project_id: id,
        request: EvaluateProjectRequest { ratings },
    };
    let client_votes = vote(vec![AccountRating {
        account: account(coordinator),
        score: Score::new(5)?,
    }]);
    reject_invalid_evaluation_targets(provider, id, client, coordinator, worker).await?;
    // Coordinator-only worker reputation does not wait for a client vote.
    success(
        provider,
        coordinator,
        vote(vec![
            AccountRating {
                account: account(client),
                score: Score::new(4)?,
            },
            AccountRating {
                account: account(worker),
                score: Score::new(3)?,
            },
        ]),
    )
    .await?;
    let after_coordinator = provider.snapshot().await?;
    let worker_score = &after_coordinator
        .workers
        .iter()
        .find(|w| w.account == account(worker))
        .ok_or("worker")?
        .worker_score;
    assert_eq!(
        worker_score.weighted_score_sum,
        u128::from(milestone.definition.requirements[0].minutes.get()) * 300
    );
    let rating_call = signed(provider, client, client_votes.clone()).await?;
    let rated = provider.execute(rating_call.clone(), NOW).await?;
    assert_eq!(rated.outcome, ExecutionOutcome::Success);
    assert_eq!(provider.execute(rating_call, NOW).await?, rated);
    assert_eq!(
        send(provider, client, client_votes).await?.outcome,
        ExecutionOutcome::Failed("project_already_evaluated".into())
    );
    success(
        provider,
        worker,
        vote(vec![AccountRating {
            account: account(coordinator),
            score: Score::new(4)?,
        }]),
    )
    .await?;
    verify_final_evaluations(provider, id, milestone, coordinator, worker, &before).await
}
async fn verify_final_evaluations(
    provider: &Provider,
    id: EntityId,
    milestone: &MilestoneView,
    coordinator: &Keypair,
    worker: &Keypair,
    before: &ProviderSnapshot,
) -> TestResult {
    use generated_contracts::{AccountRating, EvaluateProjectRequest};
    let vote = |ratings| ProviderCommand::EvaluateProject {
        project_id: id,
        request: EvaluateProjectRequest { ratings },
    };
    let evaluated = provider.snapshot().await?;
    assert_eq!(evaluated.balances, before.balances);
    assert_eq!(project(provider, id).await?.evaluations.len(), 3);
    let coordinator_score = &evaluated
        .workers
        .iter()
        .find(|w| w.account == account(coordinator))
        .ok_or("coordinator")?
        .coordinator_score;
    assert_eq!(
        coordinator_score.weighted_score_sum,
        u128::from(milestone.definition.coordinator_minutes.get()) * 900
    );
    assert_eq!(
        coordinator_score.rated_minutes,
        u64::from(milestone.definition.coordinator_minutes.get()) * 2
    );
    assert_eq!(
        send(
            provider,
            worker,
            vote(vec![AccountRating {
                account: account(coordinator),
                score: Score::new(4)?,
            }])
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("project_already_evaluated".into())
    );
    assert_eq!(provider.snapshot().await?, evaluated);

    let worker_view = evaluated
        .workers
        .iter()
        .find(|w| w.account == account(worker))
        .ok_or("worker")?;
    assert_eq!(
        worker_view.worker_score.weighted_score_sum,
        u128::from(milestone.definition.requirements[0].minutes.get()) * 300
    );
    assert_eq!(
        worker_view.worker_score.rated_minutes,
        u64::from(milestone.definition.requirements[0].minutes.get())
    );
    Ok(())
}

async fn reject_invalid_evaluation_targets(
    provider: &Provider,
    id: EntityId,
    client: &Keypair,
    coordinator: &Keypair,
    worker: &Keypair,
) -> TestResult {
    use generated_contracts::{AccountRating, EvaluateProjectRequest};
    let before = provider.snapshot().await?;
    let vote = |ratings| ProviderCommand::EvaluateProject {
        project_id: id,
        request: EvaluateProjectRequest { ratings },
    };
    for (actor, ratings) in [
        (
            client,
            vec![AccountRating {
                account: account(worker),
                score: Score::new(5)?,
            }],
        ),
        (
            client,
            vec![
                AccountRating {
                    account: account(coordinator),
                    score: Score::new(5)?,
                },
                AccountRating {
                    account: account(worker),
                    score: Score::new(5)?,
                },
            ],
        ),
        (
            worker,
            vec![AccountRating {
                account: account(client),
                score: Score::new(4)?,
            }],
        ),
        (
            coordinator,
            vec![AccountRating {
                account: account(client),
                score: Score::new(4)?,
            }],
        ),
    ] {
        assert_eq!(
            send(provider, actor, vote(ratings)).await?.outcome,
            ExecutionOutcome::Failed("evaluation_participants_mismatch".into())
        );
        assert_eq!(provider.snapshot().await?.balances, before.balances);
        assert!(project(provider, id).await?.evaluations.is_empty());
        assert_eq!(provider.snapshot().await?.workers, before.workers);
    }
    Ok(())
}
