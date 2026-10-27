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

// Drafts may be inconsistent during editing; only submission checks the global estimate.
#[allow(
    clippy::too_many_lines,
    reason = "Keep draft editing, submission rollback and resubmission in one lifecycle scenario."
)]
pub(super) async fn estimated_delivery(provider: Provider) -> TestResult {
    let (_, coordinator, _, client) = setup(&provider).await?;
    let id = success(
        &provider,
        &client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Estimated delivery".into(),
            description: String::new(),
        }),
    )
    .await?
    .created_entity_id
    .ok_or("project")?;
    let mut definition = proposal()?;
    let original = definition.milestones[0].clone();
    definition.milestones = [(2026, 53, 2027, 1), (2027, 2, 2027, 5), (2027, 6, 2027, 8)]
        .into_iter()
        .enumerate()
        .map(|(index, (sy, sw, ey, ew))| {
            Ok(MilestoneDefinition {
                key: u32::try_from(index)?,
                window: WeekWindow::new(Week::new(sy, sw)?, Week::new(ey, ew)?)?,
                ..original.clone()
            })
        })
        .collect::<TestResult<Vec<_>>>()?;
    let proposal_id = success(
        &provider,
        &coordinator,
        ProviderCommand::CreateProposal {
            project_id: id,
            proposal: definition.clone(),
        },
    )
    .await?
    .created_entity_id
    .ok_or("proposal")?;
    for (date, accepted) in [("2027-02-28", false), ("2027-03-01", true)] {
        let current = project(&provider, id).await?;
        success(
            &provider,
            &coordinator,
            ProviderCommand::SetProposalDelivery {
                project_id: id,
                proposal_id,
                request: ProposalDeliveryRequest {
                    expected_revision: current.proposals[0].revision,
                    delivery: DeliveryPreference::SpecificDate { date: date.into() },
                },
            },
        )
        .await?;
        let before = provider.snapshot().await?;
        let receipt = send(
            &provider,
            &coordinator,
            ProviderCommand::SubmitProposal {
                project_id: id,
                proposal_id,
            },
        )
        .await?;
        if accepted {
            assert_eq!(receipt.outcome, ExecutionOutcome::Success);
        } else {
            assert_eq!(
                receipt.outcome,
                ExecutionOutcome::Failed("delivery_date_before_milestones".into())
            );
            assert_eq!(provider.snapshot().await?, before);
        }
    }
    let submitted = project(&provider, id).await?;
    success(
        &provider,
        &coordinator,
        ProviderCommand::WithdrawProposal {
            project_id: id,
            proposal_id,
            expected_revision: submitted.proposals[0].revision,
        },
    )
    .await?;
    definition.milestones[1].window = WeekWindow::new(Week::new(2027, 2)?, Week::new(2027, 6)?)?;
    success(
        &provider,
        &coordinator,
        ProviderCommand::UpdateProposal {
            project_id: id,
            proposal_id,
            proposal: definition,
        },
    )
    .await?;
    let before = provider.snapshot().await?;
    assert_eq!(
        send(
            &provider,
            &coordinator,
            ProviderCommand::SubmitProposal {
                project_id: id,
                proposal_id
            }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("delivery_date_before_milestones".into())
    );
    assert_eq!(provider.snapshot().await?, before);
    let current = project(&provider, id).await?;
    success(
        &provider,
        &coordinator,
        ProviderCommand::SetProposalDelivery {
            project_id: id,
            proposal_id,
            request: ProposalDeliveryRequest {
                expected_revision: current.proposals[0].revision,
                delivery: DeliveryPreference::WithinOneMonth {},
            },
        },
    )
    .await?;
    success(
        &provider,
        &coordinator,
        ProviderCommand::SubmitProposal {
            project_id: id,
            proposal_id,
        },
    )
    .await?;
    assert_eq!(provider.snapshot().await?.balances, before.balances);
    Ok(())
}
