use generated_contracts::provider_core::ProjectCore;
use generated_contracts::provider_core::{
    MilestoneCore, ProposalContentReferenceV1, ProposalCore, ProposalTerms,
};
use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate, Weekday};

use generated_contracts::{
    AccountId32, DeliveryPreference, DomainEventKind, EntityId, MilestoneStatus, Money,
    PlanningStatus, ProposalStatus, ProviderCommand, TaskStorageView, TaskView, TeamRating,
    UnixSeconds,
};

use super::{Effect, RecordedReason, RustEngine, add_rating};
use crate::{Error, Result, require};

type Transition = (DomainEventKind, Option<EntityId>);

impl RustEngine {
    #[allow(
        clippy::too_many_lines,
        reason = "Keep explicit command dispatch together; business logic remains in domain handlers."
    )]
    pub(super) fn project_command(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        let transition = match command {
            ProviderCommand::SubmitMilestoneCommitted {
                milestone_id,
                evidence,
                worker_ratings,
                ..
            } => {
                let id = super::submissions::submit(
                    &self.context,
                    project,
                    origin,
                    *milestone_id,
                    Some(generated_contracts::EvidenceRecordReferenceV1::Committed(
                        evidence.clone(),
                    )),
                    worker_ratings.as_deref(),
                    now,
                )?;
                Ok((DomainEventKind::MilestoneCompletionRequested, Some(id)))
            }
            ProviderCommand::RejectMilestoneCommitted {
                milestone_id,
                submission_id,
                evidence,
                ..
            } => {
                super::submissions::reject(
                    project,
                    origin,
                    *milestone_id,
                    *submission_id,
                    generated_contracts::EvidenceRecordReferenceV1::Committed(evidence.clone()),
                    now,
                )?;
                Ok((
                    DomainEventKind::MilestoneCompletionRejected,
                    Some(*submission_id),
                ))
            }
            ProviderCommand::OpenDisputeCommitted {
                milestone_id,
                rejected_submission_id,
                evidence,
                ..
            } => {
                let id = self.open_case(
                    project,
                    origin,
                    *milestone_id,
                    *rejected_submission_id,
                    Some(generated_contracts::EvidenceRecordReferenceV1::Committed(
                        evidence.clone(),
                    )),
                    None,
                    now,
                )?;
                Ok((DomainEventKind::DisputeOpened, Some(id)))
            }
            ProviderCommand::RespondDisputeCommitted {
                dispute_id,
                evidence,
                ..
            } => {
                self.respond_dispute(
                    project,
                    origin,
                    *dispute_id,
                    generated_contracts::EvidenceRecordReferenceV1::Committed(evidence.clone()),
                    now,
                )?;
                Ok((DomainEventKind::DisputeResponseAdded, Some(*dispute_id)))
            }

            ProviderCommand::EvaluateProject { request, .. } => {
                self.evaluate_project(project, origin, request, now)?;
                Ok((DomainEventKind::ProjectEvaluated, Some(project.project_id)))
            }
            ProviderCommand::RejectMilestoneDelivery {
                milestone_id,
                submission_id,
                comment_id,
                ..
            } => {
                client(project, origin)?;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    milestone.status == Some(MilestoneStatus::CompletionRequested)
                        && !milestone.frozen,
                    "invalid_milestone_state",
                )?;
                super::submissions::pending(milestone, *submission_id)?.review =
                    generated_contracts::SubmissionReview::RejectedWithComment {
                        comment_id: *comment_id,
                        reviewed_by: origin,
                        reviewed_at: now,
                    };
                milestone.status = Some(MilestoneStatus::ChangesRequested);
                Ok((
                    DomainEventKind::MilestoneCompletionRejected,
                    Some(*submission_id),
                ))
            }
            ProviderCommand::RejectMilestoneCompletion {
                milestone_id,
                submission_id,
                request,
                ..
            } => {
                super::submissions::reject(
                    project,
                    origin,
                    *milestone_id,
                    *submission_id,
                    request.evidence.clone().into(),
                    now,
                )?;
                Ok((
                    DomainEventKind::MilestoneCompletionRejected,
                    Some(*submission_id),
                ))
            }
            ProviderCommand::OpenDisputeWithComment(request) => {
                let id = self.open_dispute_with_comment(project, origin, request, now)?;
                Ok((DomainEventKind::DisputeOpened, Some(id)))
            }
            ProviderCommand::OpenDispute(request) => {
                let id = self.open_dispute(project, origin, request, now)?;
                Ok((DomainEventKind::DisputeOpened, Some(id)))
            }
            ProviderCommand::RespondDispute {
                dispute_id,
                request,
                ..
            } => {
                self.respond_dispute(
                    project,
                    origin,
                    *dispute_id,
                    request.evidence.clone().into(),
                    now,
                )?;
                Ok((DomainEventKind::DisputeResponseAdded, Some(*dispute_id)))
            }
            ProviderCommand::QuotePlanning { .. }
            | ProviderCommand::AcceptPlanningQuote { .. }
            | ProviderCommand::AcceptPlanningDelivery { .. } => {
                self.planning_command(project, origin, command)
            }
            ProviderCommand::CreateProposalCore { .. }
            | ProviderCommand::UpdateProposalCore { .. }
            | ProviderCommand::DeleteProposal { .. }
            | ProviderCommand::SetProposalDelivery { .. } => {
                self.draft_proposal_command(project, origin, command)
            }
            ProviderCommand::SubmitProposal { .. }
            | ProviderCommand::ApproveExecution { .. }
            | ProviderCommand::RequestProposalChangesCore { .. }
            | ProviderCommand::WithdrawProposal { .. } => {
                self.proposal_review_command(project, origin, command)
            }
            ProviderCommand::CancelProject { .. } | ProviderCommand::DisputePlanning { .. } => {
                self.lifecycle_command(project, origin, command, now)
            }
            ProviderCommand::CreateTask { .. }
            | ProviderCommand::DeleteTask { .. }
            | ProviderCommand::EditTask { .. }
            | ProviderCommand::UpdateTaskProgress { .. } => {
                self.task_command(project, origin, command, now)
            }
            ProviderCommand::RequestMilestoneCompletionWithoutDeliverable { .. }
            | ProviderCommand::RequestMilestoneCompletion { .. }
            | ProviderCommand::AcceptMilestoneCompletion { .. }
            | ProviderCommand::SubmitMilestoneDelivery { .. }
            | ProviderCommand::AcceptMilestoneDelivery { .. } => {
                self.milestone_command(project, origin, command, now)
            }
            _ => Err(Error::bad("invalid_project_message")),
        }?;
        Ok(Effect::new(transition.0, transition.1, origin))
    }

    fn planning_command(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let kind = match command {
            ProviderCommand::QuotePlanning { .. } | ProviderCommand::AcceptPlanningQuote { .. } => {
                return Err(Error::domain("separate_planning_price_removed"));
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
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let (kind, proposal_id) = match command {
            ProviderCommand::CreateProposalCore {
                proposal, content, ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let mut proposal = proposal.clone();
                for milestone in &mut proposal.milestones {
                    milestone.allocate_budget(&self.compensation_policy)?;
                }
                let proposal_id = self.context.random_id();
                project.proposals.push(ProposalCore {
                    proposal_id,
                    revision: 0,
                    content: ProposalContentReferenceV1::Signed(content.clone()),
                    status: ProposalStatus::Draft,
                    milestones: make_milestones(&self.context, &proposal, &[])?,
                    change_request: None,
                    delivery: None,
                    compensation_policy: Some(self.compensation_policy.clone()),
                });
                (DomainEventKind::ProposalCreated, proposal_id)
            }
            ProviderCommand::UpdateProposalCore {
                proposal_id,
                proposal,
                content,
                ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let mut proposal = proposal.clone();
                for milestone in &mut proposal.milestones {
                    milestone.allocate_budget(&self.compensation_policy)?;
                }
                let current = proposal_mut(project, *proposal_id)?;
                require(
                    current.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                current.milestones =
                    make_milestones(&self.context, &proposal, &current.milestones)?;
                current.compensation_policy = Some(self.compensation_policy.clone());
                current.content = ProposalContentReferenceV1::Signed(content.clone());
                bump(&mut current.revision)?;
                (DomainEventKind::ProposalUpdated, *proposal_id)
            }
            ProviderCommand::SetProposalDelivery {
                proposal_id,
                request,
                ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                let current = proposal_mut(project, *proposal_id)?;
                require(
                    current.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                revision(current.revision, request.expected_revision)?;
                current.delivery = Some(request.delivery.clone());
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
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
    ) -> Result<Transition> {
        let (kind, proposal_id) = match command {
            ProviderCommand::WithdrawProposal {
                proposal_id,
                expected_revision,
                ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                require(
                    !project
                        .proposals
                        .iter()
                        .any(|p| p.status == ProposalStatus::Approved),
                    "execution_already_approved",
                )?;
                let proposal = proposal_mut(project, *proposal_id)?;
                revision(proposal.revision, *expected_revision)?;
                require(
                    proposal.status == ProposalStatus::PendingApproval,
                    "proposal_not_pending",
                )?;
                proposal.status = ProposalStatus::Draft;
                bump(&mut proposal.revision)?;
                if project.planning.status == PlanningStatus::Delivered {
                    project.planning.status = PlanningStatus::Accepted;
                }
                bump(&mut project.planning.revision)?;
                (DomainEventKind::ProposalWithdrawn, *proposal_id)
            }
            ProviderCommand::SubmitProposal { proposal_id, .. } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                validate_delivery_estimate(proposal)?;
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
            ProviderCommand::RequestProposalChangesCore {
                proposal_id,
                content,
                ..
            } => {
                client(project, origin)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::PendingApproval,
                    "proposal_not_pending",
                )?;
                proposal.status = ProposalStatus::Draft;
                proposal.change_request = Some(ProposalContentReferenceV1::Signed(content.clone()));
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
        project: &mut ProjectCore,
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
        let mut earlier_team = BTreeSet::new();
        for (index, milestone) in proposal.milestones.iter_mut().enumerate() {
            self.assign(project, milestone, &earlier_team)?;
            earlier_team.extend(
                milestone
                    .assignments
                    .iter()
                    .map(|assignment| assignment.worker),
            );
            milestone.status = Some(if index == 0 {
                MilestoneStatus::InProgress
            } else {
                MilestoneStatus::NotStarted
            });
        }
        proposal.status = ProposalStatus::Approved;
        bump(&mut proposal.revision)?;
        project.proposals[index] = proposal;
        project.execution_escrow = total;
        Ok(())
    }

    fn lifecycle_command(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Transition> {
        match command {
            ProviderCommand::CancelProject { request, .. } => {
                participant(project, origin)?;
                require(!project.completed, "project_completed")?;
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
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Transition> {
        let (kind, storage_id) = match command {
            ProviderCommand::DeleteTask {
                task_storage_id,
                task_id,
                expected_revision,
                ..
            } => {
                coordinator(project, origin)?;
                let proposal = project
                    .proposals
                    .iter_mut()
                    .find(|p| {
                        p.milestones
                            .iter()
                            .any(|m| m.task_storage.task_storage_id == *task_storage_id)
                    })
                    .ok_or_else(|| Error::domain("task_storage_not_found"))?;
                require(
                    proposal.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                revision(proposal.revision, *expected_revision)?;
                let storage = &mut proposal
                    .milestones
                    .iter_mut()
                    .find(|m| m.task_storage.task_storage_id == *task_storage_id)
                    .ok_or_else(Error::internal)?
                    .task_storage;
                require(
                    storage.tasks.iter().any(|t| t.task_id == *task_id),
                    "task_not_found",
                )?;
                storage.tasks.retain(|t| t.task_id != *task_id);
                (DomainEventKind::TaskDeleted, *task_storage_id)
            }
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
        project: &mut ProjectCore,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Transition> {
        match command {
            ProviderCommand::SubmitMilestoneDelivery {
                milestone_id,
                request,
                ..
            } => {
                let id = super::submissions::submit(
                    &self.context,
                    project,
                    origin,
                    *milestone_id,
                    request.deliverable.clone().map(Into::into),
                    None,
                    now,
                )?;
                Ok((DomainEventKind::MilestoneCompletionRequested, Some(id)))
            }
            ProviderCommand::AcceptMilestoneDelivery {
                milestone_id,
                submission_id,
                ..
            } => self.settle_delivery(project, origin, *milestone_id, *submission_id, now),
            ProviderCommand::RequestMilestoneCompletion {
                milestone_id,
                request,
                ..
            } => {
                let id = super::submissions::submit(
                    &self.context,
                    project,
                    origin,
                    *milestone_id,
                    Some(request.deliverable.clone().into()),
                    Some(&request.worker_ratings),
                    now,
                )?;
                Ok((DomainEventKind::MilestoneCompletionRequested, Some(id)))
            }
            ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
                milestone_id,
                worker_ratings,
                ..
            } => {
                let id = super::submissions::submit(
                    &self.context,
                    project,
                    origin,
                    *milestone_id,
                    None,
                    Some(worker_ratings),
                    now,
                )?;
                Ok((DomainEventKind::MilestoneCompletionRequested, Some(id)))
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
                super::submissions::pending(milestone, request.submission_id)?;

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

                    add_rating(
                        &mut self.worker_mut(assignment.worker)?.worker_score,
                        contribution,
                        requirement.minutes,
                    )?;
                }
                self.settle_delivery(project, origin, *milestone_id, request.submission_id, now)
            }
            _ => Err(Error::bad("invalid_project_message")),
        }
    }
    fn settle_delivery(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        milestone_id: EntityId,
        submission_id: EntityId,
        now: UnixSeconds,
    ) -> Result<Transition> {
        client(project, origin)?;
        let coordinator = project.coordinator;
        let milestone = milestone_mut(project, milestone_id)?;
        require(
            milestone.status == Some(MilestoneStatus::CompletionRequested) && !milestone.frozen,
            "invalid_milestone_state",
        )?;
        super::submissions::pending(milestone, submission_id)?;
        let total = milestone.definition.total()?;
        self.credit(coordinator, milestone.definition.coordinator_fee)?;
        for requirement in &milestone.definition.requirements {
            let assignment = milestone
                .assignments
                .iter()
                .find(|a| a.requirement_key == requirement.key)
                .ok_or_else(Error::internal)?;
            self.credit(assignment.worker, requirement.budget)?;
        }
        super::submissions::pending(milestone, submission_id)?.review =
            generated_contracts::SubmissionReview::Accepted {
                reviewed_by: origin,
                reviewed_at: now,
            };
        milestone.status = Some(MilestoneStatus::Completed);
        project.execution_escrow = project.execution_escrow.checked_sub(total)?;
        activate_next_milestone(project, milestone_id)?;
        Ok((DomainEventKind::MilestoneCompleted, Some(milestone_id)))
    }
}

fn activate_next_milestone(project: &mut ProjectCore, completed_id: EntityId) -> Result<()> {
    let proposal = project
        .proposals
        .iter_mut()
        .find(|proposal| {
            proposal.status == ProposalStatus::Approved
                && proposal
                    .milestones
                    .iter()
                    .any(|milestone| milestone.milestone_id == completed_id)
        })
        .ok_or_else(Error::internal)?;
    let index = proposal
        .milestones
        .iter()
        .position(|milestone| milestone.milestone_id == completed_id)
        .ok_or_else(Error::internal)?;
    if let Some(next) = proposal.milestones.get_mut(index + 1) {
        require(
            next.status == Some(MilestoneStatus::NotStarted),
            "invalid_milestone_state",
        )?;
        next.status = Some(MilestoneStatus::InProgress);
    } else {
        project.completed = true;
    }
    Ok(())
}

fn client(project: &ProjectCore, origin: AccountId32) -> Result<()> {
    require(project.client == origin, "client_required")
}

fn coordinator(project: &ProjectCore, origin: AccountId32) -> Result<()> {
    require(project.coordinator == origin, "coordinator_required")
}

fn participant(project: &ProjectCore, origin: AccountId32) -> Result<()> {
    require(
        project.client == origin || project.coordinator == origin,
        "project_party_required",
    )
}

fn planning_active(project: &ProjectCore) -> Result<()> {
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

fn proposal_mut(project: &mut ProjectCore, id: EntityId) -> Result<&mut ProposalCore> {
    project
        .proposals
        .iter_mut()
        .find(|proposal| proposal.proposal_id == id)
        .ok_or_else(|| Error::domain("proposal_not_found"))
}

pub(super) fn milestone_mut(project: &mut ProjectCore, id: EntityId) -> Result<&mut MilestoneCore> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .find(|milestone| milestone.milestone_id == id)
        .ok_or_else(|| Error::domain("milestone_not_found"))
}

fn storage_mut(project: &mut ProjectCore, id: EntityId) -> Result<&mut TaskStorageView> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .map(|milestone| &mut milestone.task_storage)
        .find(|storage| storage.task_storage_id == id)
        .ok_or_else(|| Error::domain("task_storage_not_found"))
}

fn task_mut(project: &mut ProjectCore, storage: EntityId, id: u32) -> Result<&mut TaskView> {
    storage_mut(project, storage)?
        .tasks
        .iter_mut()
        .find(|task| task.task_id == id)
        .ok_or_else(|| Error::domain("task_not_found"))
}

pub(super) fn proposal_definition(proposal: &ProposalCore) -> ProposalTerms {
    ProposalTerms {
        milestones: proposal
            .milestones
            .iter()
            .map(|m| m.definition.clone())
            .collect(),
    }
}

fn make_milestones(
    context: &super::execution::EngineContext,
    proposal: &ProposalTerms,
    previous: &[MilestoneCore],
) -> Result<Vec<MilestoneCore>> {
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
                let milestone_id = context.random_id();
                Ok(MilestoneCore {
                    milestone_id,
                    definition: definition.clone(),
                    status: None,
                    assignments: Vec::new(),
                    worker_ratings: Vec::new(),
                    submissions: Vec::new(),
                    task_storage: TaskStorageView {
                        task_storage_id: context.random_id(),
                        milestone_id,
                        tasks: Vec::new(),
                    },
                    frozen: false,
                })
            }
        })
        .collect()
}

fn validate_delivery_estimate(proposal: &ProposalCore) -> Result<()> {
    let Some(DeliveryPreference::SpecificDate { date }) = &proposal.delivery else {
        return Ok(());
    };
    let monday = |week: generated_contracts::Week| {
        NaiveDate::from_isoywd_opt(week.iso_year(), u32::from(week.week()), Weekday::Mon)
            .ok_or_else(|| Error::bad("invalid_delivery_schedule"))
    };
    let first = proposal
        .milestones
        .first()
        .ok_or_else(|| Error::bad("empty_milestones"))?;
    let start = monday(first.definition.window.start())?;
    let days = proposal
        .milestones
        .iter()
        .try_fold(0_i64, |sum, milestone| {
            let window = milestone.definition.window;
            let duration = monday(window.end())?
                .signed_duration_since(monday(window.start())?)
                .num_days();
            sum.checked_add(duration)
                .and_then(|total| total.checked_add(7))
                .ok_or_else(|| Error::bad("invalid_delivery_schedule"))
        })?;
    let minimum = Duration::try_days(days)
        .and_then(|duration| start.checked_add_signed(duration))
        .ok_or_else(|| Error::bad("invalid_delivery_schedule"))?;
    let estimated = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| Error::bad("invalid_delivery_date"))?;
    require(estimated >= minimum, "delivery_date_before_milestones")
}
