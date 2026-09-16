use std::collections::BTreeSet;

use generated_contracts::{
    AccountId32, DomainEventKind, EntityId, MilestoneStatus, MilestoneView, Minutes, Money,
    PlanningStatus, ProjectView, ProposalDefinition, ProposalStatus, ProposalView, ProviderCommand,
    ReservationView, TaskStorageView, TaskView, TeamRating, UnixSeconds,
};

use super::{Effect, RecordedReason, State, add_rating};
use crate::{Error, Result, calendar, random_id, require};

type Transition = (DomainEventKind, Option<EntityId>);

impl State {
    pub(super) fn project_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        let transition = match command {
            ProviderCommand::QuotePlanning { .. }
            | ProviderCommand::AcceptPlanningQuote { .. }
            | ProviderCommand::AcceptPlanningDelivery { .. } => {
                self.planning_command(project, origin, command)
            }
            ProviderCommand::CreateProposal { .. }
            | ProviderCommand::UpdateProposal { .. }
            | ProviderCommand::DeleteProposal { .. } => {
                self.draft_proposal_command(project, origin, command)
            }
            ProviderCommand::SubmitProposal { .. }
            | ProviderCommand::ApproveExecution { .. }
            | ProviderCommand::RequestProposalChanges { .. } => {
                self.proposal_review_command(project, origin, command)
            }
            ProviderCommand::CancelProject { .. }
            | ProviderCommand::DisputePlanning { .. }
            | ProviderCommand::DisputeMilestone { .. } => {
                self.lifecycle_command(project, origin, command, now)
            }
            ProviderCommand::CreateTask { .. }
            | ProviderCommand::EditTask { .. }
            | ProviderCommand::UpdateTaskProgress { .. } => {
                self.task_command(project, origin, command, now)
            }
            ProviderCommand::RequestMilestoneCompletion { .. }
            | ProviderCommand::AcceptMilestoneCompletion { .. } => {
                self.milestone_command(project, origin, command)
            }
            _ => Err(Error::bad("invalid_project_message")),
        }?;
        Ok(Effect::new(transition.0, transition.1, origin))
    }

    fn planning_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let kind = match command {
            ProviderCommand::QuotePlanning { quote, .. } => {
                coordinator(project, origin)?;
                require(
                    matches!(
                        project.planning.status,
                        PlanningStatus::AwaitingQuote | PlanningStatus::Quoted
                    ),
                    "invalid_planning_state",
                )?;
                project.planning.quote = Some(quote.clone());
                project.planning.status = PlanningStatus::Quoted;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningQuoted
            }
            ProviderCommand::AcceptPlanningQuote {
                expected_revision, ..
            } => {
                client(project, origin)?;
                revision(project.planning.revision, *expected_revision)?;
                require(
                    project.planning.status == PlanningStatus::Quoted && !project.planning.frozen,
                    "invalid_planning_state",
                )?;
                let quote = project
                    .planning
                    .quote
                    .as_ref()
                    .ok_or_else(Error::internal)?;
                self.debit(project.client, quote.fee)?;
                calendar::reserve(
                    &mut self.worker_mut(project.coordinator)?.calendar,
                    quote.window,
                    quote.minutes,
                    &ReservationView {
                        project_id: project.project_id,
                        milestone_id: None,
                        requirement_key: None,
                        week: quote.window.start(),
                        minutes: Minutes::ZERO,
                    },
                )?;
                project.planning.escrow = quote.fee;
                project.planning.status = PlanningStatus::Accepted;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningAccepted
            }
            ProviderCommand::AcceptPlanningDelivery {
                expected_revision, ..
            } => {
                client(project, origin)?;
                revision(project.planning.revision, *expected_revision)?;
                require(
                    project.planning.status == PlanningStatus::Delivered
                        && !project.planning.frozen,
                    "invalid_planning_state",
                )?;
                self.credit(project.coordinator, project.planning.escrow)?;
                project.planning.escrow = Money::ZERO;
                project.planning.status = PlanningStatus::Completed;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningCompleted
            }
            _ => return Err(Error::bad("invalid_project_message")),
        };
        Ok((kind, Some(project.project_id)))
    }

    fn draft_proposal_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let (kind, proposal_id) = match command {
            ProviderCommand::CreateProposal { proposal, .. } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let proposal_id = random_id()?;
                project.proposals.push(ProposalView {
                    proposal_id,
                    revision: 0,
                    title: proposal.title.clone(),
                    description: proposal.description.clone(),
                    status: ProposalStatus::Draft,
                    milestones: make_milestones(proposal, &[])?,
                    change_request: None,
                });
                (DomainEventKind::ProposalCreated, proposal_id)
            }
            ProviderCommand::UpdateProposal {
                proposal_id,
                proposal,
                ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let current = proposal_mut(project, *proposal_id)?;
                require(
                    current.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                current.milestones = make_milestones(proposal, &current.milestones)?;
                current.title.clone_from(&proposal.title);
                current.description.clone_from(&proposal.description);
                bump(&mut current.revision)?;
                (DomainEventKind::ProposalUpdated, *proposal_id)
            }
            ProviderCommand::DeleteProposal { proposal_id, .. } => {
                coordinator(project, origin)?;
                require(
                    proposal_mut(project, *proposal_id)?.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                project
                    .proposals
                    .retain(|proposal| proposal.proposal_id != *proposal_id);
                (DomainEventKind::ProposalDeleted, *proposal_id)
            }
            _ => return Err(Error::bad("invalid_project_message")),
        };
        Ok((kind, Some(proposal_id)))
    }

    fn proposal_review_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let (kind, proposal_id) = match command {
            ProviderCommand::SubmitProposal { proposal_id, .. } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                proposal.status = ProposalStatus::PendingApproval;
                bump(&mut proposal.revision)?;
                if project.planning.status != PlanningStatus::Completed {
                    project.planning.status = PlanningStatus::Delivered;
                }
                bump(&mut project.planning.revision)?;
                (DomainEventKind::ProposalSubmitted, *proposal_id)
            }
            ProviderCommand::ApproveExecution {
                proposal_id,
                expected_revision,
                ..
            } => {
                self.approve_execution(project, origin, *proposal_id, *expected_revision)?;
                (DomainEventKind::ExecutionApproved, *proposal_id)
            }
            ProviderCommand::RequestProposalChanges {
                proposal_id,
                request,
                ..
            } => {
                client(project, origin)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::PendingApproval,
                    "proposal_not_pending",
                )?;
                proposal.status = ProposalStatus::Draft;
                proposal.change_request = Some(request.reference.clone());
                bump(&mut proposal.revision)?;
                if project.planning.status == PlanningStatus::Delivered {
                    project.planning.status = PlanningStatus::Accepted;
                }
                bump(&mut project.planning.revision)?;
                (DomainEventKind::ProposalChangesRequested, *proposal_id)
            }
            _ => return Err(Error::bad("invalid_project_message")),
        };
        Ok((kind, Some(proposal_id)))
    }

    fn approve_execution(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        proposal_id: EntityId,
        expected_revision: u64,
    ) -> Result<()> {
        client(project, origin)?;
        require(
            project.planning.status == PlanningStatus::Completed && !project.planning.frozen,
            "planning_not_accepted",
        )?;
        require(
            !project
                .proposals
                .iter()
                .any(|proposal| proposal.status == ProposalStatus::Approved),
            "execution_already_approved",
        )?;
        let index = project
            .proposals
            .iter()
            .position(|proposal| proposal.proposal_id == proposal_id)
            .ok_or_else(|| Error::domain("proposal_not_found"))?;
        let mut proposal = project.proposals[index].clone();
        revision(proposal.revision, expected_revision)?;
        require(
            proposal.status == ProposalStatus::PendingApproval,
            "proposal_not_pending",
        )?;
        let total = proposal_definition(&proposal).total()?;
        self.debit(project.client, total)?;
        for milestone in &mut proposal.milestones {
            self.assign(project, milestone)?;
        }
        proposal.status = ProposalStatus::Approved;
        bump(&mut proposal.revision)?;
        project.proposals[index] = proposal;
        project.execution_escrow = total;
        Ok(())
    }

    fn lifecycle_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Transition> {
        match command {
            ProviderCommand::CancelProject { request, .. } => {
                participant(project, origin)?;
                self.record_reason(
                    project.project_id,
                    None,
                    origin,
                    request.reason.clone(),
                    now,
                    DomainEventKind::ProjectCancelled,
                );
                project.cancelled = true;
                project.planning.frozen = project.planning.escrow != Money::ZERO;
                bump(&mut project.planning.revision)?;
                for proposal in &mut project.proposals {
                    proposal.status = ProposalStatus::Cancelled;
                    bump(&mut proposal.revision)?;
                    for milestone in &mut proposal.milestones {
                        milestone.frozen = milestone.status != Some(MilestoneStatus::Completed);
                    }
                }
                Ok((DomainEventKind::ProjectCancelled, Some(project.project_id)))
            }
            ProviderCommand::DisputePlanning { request, .. } => {
                participant(project, origin)?;
                require(
                    matches!(
                        project.planning.status,
                        PlanningStatus::Accepted | PlanningStatus::Delivered
                    ),
                    "invalid_planning_state",
                )?;
                project.planning.status = PlanningStatus::Disputed;
                project.planning.frozen = true;
                self.record_reason(
                    project.project_id,
                    None,
                    origin,
                    request.reason.clone(),
                    now,
                    DomainEventKind::PlanningDisputed,
                );
                bump(&mut project.planning.revision)?;
                Ok((DomainEventKind::PlanningDisputed, Some(project.project_id)))
            }
            ProviderCommand::DisputeMilestone {
                milestone_id,
                request,
                ..
            } => {
                participant(project, origin)?;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    matches!(
                        milestone.status,
                        Some(MilestoneStatus::InProgress | MilestoneStatus::CompletionRequested)
                    ),
                    "invalid_milestone_state",
                )?;
                milestone.status = Some(MilestoneStatus::Disputed);
                milestone.frozen = true;
                self.record_reason(
                    project.project_id,
                    Some(*milestone_id),
                    origin,
                    request.reason.clone(),
                    now,
                    DomainEventKind::MilestoneDisputed,
                );
                Ok((DomainEventKind::MilestoneDisputed, Some(*milestone_id)))
            }
            _ => Err(Error::bad("invalid_project_message")),
        }
    }

    fn record_reason(
        &mut self,
        project_id: EntityId,
        milestone_id: Option<EntityId>,
        origin: AccountId32,
        reason: String,
        occurred_at: UnixSeconds,
        kind: DomainEventKind,
    ) {
        self.reasons.push(RecordedReason {
            project_id,
            milestone_id,
            origin,
            reason,
            occurred_at,
            kind,
        });
    }

    fn task_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Transition> {
        let (kind, storage_id) = match command {
            ProviderCommand::CreateTask {
                task_storage_id,
                task,
                ..
            } => {
                coordinator(project, origin)?;
                self.task_assignees(task)?;
                let task_id = self.next_task_id;
                self.next_task_id = task_id
                    .checked_add(1)
                    .ok_or_else(|| Error::domain("task_keys_exhausted"))?;
                storage_mut(project, *task_storage_id)?
                    .tasks
                    .push(TaskView {
                        task_id,
                        reporter: origin,
                        created_at: now,
                        updated_at: now,
                        task: task.clone(),
                    });
                (DomainEventKind::TaskCreated, *task_storage_id)
            }
            ProviderCommand::EditTask {
                task_storage_id,
                task_id,
                task,
                ..
            } => {
                coordinator(project, origin)?;
                self.task_assignees(task)?;
                let current = task_mut(project, *task_storage_id, *task_id)?;
                current.task = task.clone();
                current.updated_at = now;
                (DomainEventKind::TaskUpdated, *task_storage_id)
            }
            ProviderCommand::UpdateTaskProgress {
                task_storage_id,
                task_id,
                progress,
                ..
            } => {
                require(origin != project.client, "client_read_only")?;
                let is_coordinator = project.coordinator == origin;
                let current = task_mut(project, *task_storage_id, *task_id)?;
                require(
                    is_coordinator || current.task.assignees.contains(&origin),
                    "task_assignee_required",
                )?;
                current.task.status = progress.status;
                current.task.logged_minutes = progress.logged_minutes;
                current.updated_at = now;
                (DomainEventKind::TaskUpdated, *task_storage_id)
            }
            _ => return Err(Error::bad("invalid_project_message")),
        };
        Ok((kind, Some(storage_id)))
    }

    fn milestone_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        match command {
            ProviderCommand::RequestMilestoneCompletion {
                milestone_id,
                request,
                ..
            } => {
                coordinator(project, origin)?;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    milestone.status == Some(MilestoneStatus::InProgress) && !milestone.frozen,
                    "invalid_milestone_state",
                )?;
                let assigned: BTreeSet<_> = milestone
                    .assignments
                    .iter()
                    .map(|item| item.worker)
                    .collect();
                let rated: BTreeSet<_> = request
                    .worker_ratings
                    .iter()
                    .map(|item| item.worker)
                    .collect();
                require(
                    assigned == rated && rated.len() == request.worker_ratings.len(),
                    "worker_ratings_mismatch",
                )?;
                milestone.worker_ratings.clone_from(&request.worker_ratings);
                milestone.status = Some(MilestoneStatus::CompletionRequested);
                Ok((
                    DomainEventKind::MilestoneCompletionRequested,
                    Some(*milestone_id),
                ))
            }
            ProviderCommand::AcceptMilestoneCompletion {
                milestone_id,
                request,
                ..
            } => {
                client(project, origin)?;
                let coordinator = project.coordinator;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    milestone.status == Some(MilestoneStatus::CompletionRequested)
                        && !milestone.frozen,
                    "invalid_milestone_state",
                )?;
                let total = milestone.definition.total()?;
                self.credit(coordinator, milestone.definition.coordinator_fee)?;
                add_rating(
                    &mut self.worker_mut(coordinator)?.coordinator_score,
                    u16::from(request.coordinator_score.get()) * 100,
                    milestone.definition.coordinator_minutes,
                )?;
                for requirement in &milestone.definition.requirements {
                    let assignment = milestone
                        .assignments
                        .iter()
                        .find(|assignment| assignment.requirement_key == requirement.key)
                        .ok_or_else(Error::internal)?;
                    let rating = milestone
                        .worker_ratings
                        .iter()
                        .find(|rating| rating.worker == assignment.worker)
                        .ok_or_else(Error::internal)?;
                    let contribution = match request.team_rating {
                        TeamRating::Client(score) => self.score_policy.blend(rating.score, score),
                        TeamRating::DelegateToCoordinator => u16::from(rating.score.get()) * 100,
                    };
                    self.credit(assignment.worker, requirement.budget)?;
                    add_rating(
                        &mut self.worker_mut(assignment.worker)?.worker_score,
                        contribution,
                        requirement.minutes,
                    )?;
                }
                milestone.status = Some(MilestoneStatus::Completed);
                project.execution_escrow = project.execution_escrow.checked_sub(total)?;
                Ok((DomainEventKind::MilestoneCompleted, Some(*milestone_id)))
            }
            _ => Err(Error::bad("invalid_project_message")),
        }
    }
}

fn client(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(project.client == origin, "client_required")
}

fn coordinator(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(project.coordinator == origin, "coordinator_required")
}

fn participant(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(
        project.client == origin || project.coordinator == origin,
        "project_party_required",
    )
}

fn planning_active(project: &ProjectView) -> Result<()> {
    require(
        matches!(
            project.planning.status,
            PlanningStatus::Accepted | PlanningStatus::Delivered | PlanningStatus::Completed
        ) && !project.planning.frozen,
        "planning_not_accepted",
    )
}

fn revision(current: u64, expected: u64) -> Result<()> {
    require(current == expected, "revision_conflict")
}

fn bump(value: &mut u64) -> Result<()> {
    *value = value
        .checked_add(1)
        .ok_or_else(|| Error::domain("revision_exhausted"))?;
    Ok(())
}

fn proposal_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut ProposalView> {
    project
        .proposals
        .iter_mut()
        .find(|proposal| proposal.proposal_id == id)
        .ok_or_else(|| Error::domain("proposal_not_found"))
}

fn milestone_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut MilestoneView> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .find(|milestone| milestone.milestone_id == id)
        .ok_or_else(|| Error::domain("milestone_not_found"))
}

fn storage_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut TaskStorageView> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .map(|milestone| &mut milestone.task_storage)
        .find(|storage| storage.task_storage_id == id)
        .ok_or_else(|| Error::domain("task_storage_not_found"))
}

fn task_mut(project: &mut ProjectView, storage: EntityId, id: u32) -> Result<&mut TaskView> {
    storage_mut(project, storage)?
        .tasks
        .iter_mut()
        .find(|task| task.task_id == id)
        .ok_or_else(|| Error::domain("task_not_found"))
}

pub(super) fn proposal_definition(proposal: &ProposalView) -> ProposalDefinition {
    ProposalDefinition {
        title: proposal.title.clone(),
        description: proposal.description.clone(),
        milestones: proposal
            .milestones
            .iter()
            .map(|milestone| milestone.definition.clone())
            .collect(),
    }
}

fn make_milestones(
    proposal: &ProposalDefinition,
    previous: &[MilestoneView],
) -> Result<Vec<MilestoneView>> {
    proposal
        .milestones
        .iter()
        .map(|definition| {
            if let Some(existing) = previous
                .iter()
                .find(|milestone| milestone.definition.key == definition.key)
            {
                let mut milestone = existing.clone();
                milestone.definition = definition.clone();
                Ok(milestone)
            } else {
                let milestone_id = random_id()?;
                Ok(MilestoneView {
                    milestone_id,
                    definition: definition.clone(),
                    status: None,
                    assignments: Vec::new(),
                    worker_ratings: Vec::new(),
                    task_storage: TaskStorageView {
                        task_storage_id: random_id()?,
                        milestone_id,
                        tasks: Vec::new(),
                    },
                    frozen: false,
                })
            }
        })
        .collect()
}
