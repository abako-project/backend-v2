use std::collections::BTreeSet;

use generated_contracts::{
    EntityId, MilestoneStatus, Money, PlanningStatus, UnsignedContractCallV1, WorkerMode,
};

use super::{State, project::proposal_definition, validate_score};
use crate::{Error, Result, calendar, require};

impl State {
    /// Validate the complete persisted aggregate at its serialization boundary.
    pub(super) fn validate(&self) -> Result<()> {
        let mut identities = self.validate_workers()?;
        self.validate_projects(&mut identities)?;
        self.validate_disputes(&mut identities)?;
        self.validate_history()
    }

    fn validate_workers(&self) -> Result<BTreeSet<EntityId>> {
        require(
            self.roles
                .get(&1)
                .is_some_and(|role| role.name == "coordinator" && role.fixed),
            "invalid_catalog_state",
        )?;
        let mut identities = BTreeSet::new();
        for (account, worker) in &self.workers {
            require(
                *account == worker.account
                    && worker.calendar.owner == *account
                    && identities.insert(worker.calendar.calendar_id),
                "invalid_worker_state",
            )?;
            require(
                worker.mode != WorkerMode::Coordinator || worker.coordinator_eligible,
                "invalid_worker_state",
            )?;
            self.qualifications(&worker.qualifications)?;
            calendar::validate(&worker.calendar)?;
            validate_score(&worker.worker_score)?;
            validate_score(&worker.coordinator_score)?;
        }
        Ok(identities)
    }

    fn validate_projects(&self, identities: &mut BTreeSet<EntityId>) -> Result<()> {
        let mut total = self
            .balances
            .values()
            .try_fold(Money::ZERO, |sum, amount| sum.checked_add(*amount))?;
        for (id, project) in &self.projects {
            require(
                *id == project.project_id && identities.insert(*id),
                "invalid_project_state",
            )?;
            require(
                self.workers.contains_key(&project.coordinator),
                "invalid_coordinator_state",
            )?;
            total = total
                .checked_add(project.planning.escrow)?
                .checked_add(project.execution_escrow)?;
            let mut expected_execution = Money::ZERO;
            for proposal in &project.proposals {
                require(identities.insert(proposal.proposal_id), "duplicate_entity")?;
                proposal_definition(proposal).validate()?;
                for milestone in &proposal.milestones {
                    require(
                        identities.insert(milestone.milestone_id)
                            && identities.insert(milestone.task_storage.task_storage_id)
                            && milestone.task_storage.milestone_id == milestone.milestone_id,
                        "invalid_storage_state",
                    )?;
                    for task in &milestone.task_storage.tasks {
                        task.task.validate()?;
                    }
                    if milestone.status.is_some() {
                        require(
                            milestone.assignments.len() == milestone.definition.requirements.len(),
                            "invalid_assignment_state",
                        )?;
                        require(
                            milestone
                                .assignments
                                .iter()
                                .map(|assignment| assignment.worker)
                                .collect::<BTreeSet<_>>()
                                .len()
                                == milestone.assignments.len(),
                            "invalid_assignment_state",
                        )?;
                        for requirement in &milestone.definition.requirements {
                            require(
                                milestone.assignments.iter().any(|assignment| {
                                    assignment.requirement_key == requirement.key
                                        && self.workers.contains_key(&assignment.worker)
                                }),
                                "invalid_assignment_state",
                            )?;
                        }
                        if milestone.status != Some(MilestoneStatus::Completed) {
                            expected_execution =
                                expected_execution.checked_add(milestone.definition.total()?)?;
                        }
                    } else {
                        require(milestone.assignments.is_empty(), "invalid_assignment_state")?;
                    }
                }
            }
            require(
                project.execution_escrow == expected_execution,
                "escrow_mismatch",
            )?;
            let planning_fee = project
                .planning
                .quote
                .as_ref()
                .map_or(Money::ZERO, |quote| quote.fee);
            match project.planning.status {
                PlanningStatus::AwaitingQuote
                | PlanningStatus::Quoted
                | PlanningStatus::Completed => {
                    require(project.planning.escrow == Money::ZERO, "escrow_mismatch")?;
                }
                PlanningStatus::Accepted | PlanningStatus::Delivered | PlanningStatus::Disputed => {
                    require(project.planning.escrow == planning_fee, "escrow_mismatch")?;
                }
            }
        }
        require(total == self.minted_units, "supply_mismatch")?;
        Ok(())
    }

    fn validate_history(&self) -> Result<()> {
        for (index, event) in self.events.iter().enumerate() {
            require(
                event.provider_instance_id == self.info.provider_instance_id
                    && event.cursor == u64::try_from(index).map_err(|_| Error::internal())? + 1,
                "invalid_event_state",
            )?;
        }
        for (operation_id, recorded) in &self.receipts {
            let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
            require(
                call.operation_id == *operation_id
                    && recorded.receipt.operation_id == *operation_id
                    && call.provider_instance_id == self.info.provider_instance_id
                    && recorded.receipt.origin == call.origin
                    && recorded.receipt.nonce == call.nonce
                    && self.nonce(call.origin).nonce > call.nonce,
                "invalid_receipt_state",
            )?;
        }
        Ok(())
    }
}
