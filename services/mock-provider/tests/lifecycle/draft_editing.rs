use super::*;

#[allow(clippy::too_many_lines)]
pub(super) async fn editing(provider: Provider) -> TestResult {
    let (_, coordinator, _, client) = setup(&provider).await?;
    let (id, proposal_id) = delivered_plan(&provider, &coordinator, &client, proposal()?).await?;
    let before = project(&provider, id).await?;
    let balances = provider.snapshot().await?.balances;
    let revision = before.proposals[0].revision;
    let request = ProviderCommand::WithdrawProposal {
        project_id: id,
        proposal_id,
        expected_revision: revision,
    };
    assert_eq!(
        send(&provider, &client, request.clone()).await?.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    success(&provider, &coordinator, request).await?;
    let draft = project(&provider, id).await?;
    assert_eq!(draft.proposals[0].status, ProposalStatus::Draft);
    assert_eq!(
        draft.proposals[0].milestones,
        before.proposals[0].milestones
    );
    assert_eq!(draft.planning.status, PlanningStatus::Completed);
    assert_eq!(draft.planning.escrow, before.planning.escrow);
    assert_eq!(provider.snapshot().await?.balances, balances);
    let request = ProposalDeliveryRequest {
        expected_revision: draft.proposals[0].revision,
        delivery: DeliveryPreference::SpecificDate {
            date: "2028-02-29".into(),
        },
    };
    success(
        &provider,
        &coordinator,
        ProviderCommand::SetProposalDelivery {
            project_id: id,
            proposal_id,
            request: request.clone(),
        },
    )
    .await?;
    assert_eq!(
        send(
            &provider,
            &coordinator,
            ProviderCommand::SetProposalDelivery {
                project_id: id,
                proposal_id,
                request: request.clone()
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("revision_conflict".into())
    );
    let draft = project(&provider, id).await?;
    assert_eq!(draft.proposals[0].delivery, Some(request.delivery));
    let storage = &draft.proposals[0].milestones[0].task_storage;
    let delete = ProviderCommand::DeleteTask {
        project_id: id,
        task_storage_id: storage.task_storage_id,
        task_id: storage.tasks[0].task_id,
        expected_revision: draft.proposals[0].revision,
    };
    assert_eq!(
        send(&provider, &client, delete.clone()).await?.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    success(&provider, &coordinator, delete.clone()).await?;
    let empty = project(&provider, id).await?;
    assert_eq!(
        empty.proposals[0].milestones[0]
            .task_storage
            .task_storage_id,
        storage.task_storage_id
    );
    assert!(
        empty.proposals[0].milestones[0]
            .task_storage
            .tasks
            .is_empty()
    );
    success(
        &provider,
        &coordinator,
        ProviderCommand::SubmitProposal {
            project_id: id,
            proposal_id,
        },
    )
    .await?;
    assert_eq!(
        send(&provider, &coordinator, delete).await?.outcome,
        ExecutionOutcome::Failed("proposal_not_draft".into())
    );
    assert_eq!(
        approve(&provider, &client, id, proposal_id).await?.outcome,
        ExecutionOutcome::Success
    );
    let approved = project(&provider, id).await?;
    assert_ne!(
        send(
            &provider,
            &coordinator,
            ProviderCommand::WithdrawProposal {
                project_id: id,
                proposal_id,
                expected_revision: approved.proposals[0].revision
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Success
    );
    Ok(())
}
