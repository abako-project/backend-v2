//! Read one committed revision. No native engine, replay or state comparison is involved.
use super::*;
use generated_contracts::{provider_core::*, *};
use mock_provider::{Error as ApiError, Result as ApiResult};

pub(crate) struct Reader {
    stored: Stored,
    state: directory::State,
}
impl Reader {
    pub(super) fn new(stored: Stored, mode: Mode) -> Result<Self, Error> {
        let state = directory::decode(&stored, mode)?;
        Ok(Self { stored, state })
    }
    fn facts(&self) -> ApiResult<facts::Facts> {
        self.stored
            .checkpoint
            .clone()
            .restore()
            .map_err(|_| ApiError::unavailable())
            .and_then(|mut ext| {
                facts::read(&mut ext, &self.state).map_err(|_| ApiError::unavailable())
            })
    }
    pub(crate) fn info(&self) -> ProviderInfo {
        self.state.shell.info.clone()
    }
    pub(crate) fn task_allocation(
        &self,
        operation: OperationId,
    ) -> ApiResult<Option<TaskAllocation>> {
        let allocation = self.state.shell.task_allocation(operation)?;
        if let Some(allocated) = &allocation {
            let mut ext = self
                .stored
                .checkpoint
                .clone()
                .restore()
                .map_err(|_| ApiError::unavailable())?;
            let next =
                facts::next_task_id(&mut ext, &self.state).map_err(|_| ApiError::unavailable())?;
            if allocated.task_id >= next {
                return Err(ApiError::unavailable());
            }
        }
        Ok(allocation)
    }
    pub(crate) fn snapshot(&self) -> ApiResult<CoreSnapshot> {
        let facts = self.facts()?;
        Ok(CoreSnapshot {
            schema_version: 5,
            info: self.info(),
            catalog: CatalogCore {
                roles: facts.roles.into_values().collect(),
                skills: facts.skills.into_values().collect(),
                skill_roles: facts
                    .skill_metadata
                    .into_iter()
                    .map(|(skill_id, metadata)| SkillRoleCore {
                        skill_id,
                        role_ids: metadata.role_ids,
                        origin: metadata.origin,
                    })
                    .collect(),
                score_policy: facts.score_policy,
                compensation_policy: facts.compensation_policy,
            },
            workers: facts.workers.into_values().collect(),
            projects: facts.projects.into_values().collect(),
            balances: facts
                .balances
                .into_iter()
                .map(|(account, available)| BalanceView {
                    account,
                    asset_id: 1,
                    available,
                })
                .collect(),
        })
    }
    pub(crate) fn task_state(&self) -> ApiResult<TaskState> {
        let facts = self.facts()?;
        let mut storages: Vec<_> = facts
            .projects
            .into_values()
            .flat_map(|project| {
                let project_id = project.project_id;
                project.proposals.into_iter().flat_map(move |proposal| {
                    let proposal_id = proposal.proposal_id;
                    let proposal_revision = proposal.revision;
                    proposal
                        .milestones
                        .into_iter()
                        .map(move |milestone| TaskStorageRecord {
                            project_id,
                            proposal_id,
                            proposal_revision,
                            storage: milestone.task_storage,
                        })
                })
            })
            .collect();
        storages.sort_by_key(|record| record.storage.task_storage_id);
        let has_task_history = self.state.shell.receipts.values().any(|record| {
            UnsignedContractCallV1::decode_signable(&record.bytes).map_or(true, |call| {
                matches!(
                    call.command,
                    ProviderCommand::CreateTaskCore { .. }
                        | ProviderCommand::SetTaskAssigneesCore { .. }
                        | ProviderCommand::DeleteTask { .. }
                )
            })
        });
        Ok(TaskState {
            schema_version: 2,
            generation: self.info().provider_instance_id,
            next_task_id: facts.next_task_id,
            has_task_history,
            storages,
        })
    }
    pub(crate) fn lifecycle_reasons(&self, after: u64, limit: usize) -> ApiResult<ReasonPageV1> {
        if !(1..=256).contains(&limit) {
            return Err(ApiError::domain("invalid_reason_limit"));
        }
        let facts = self.facts()?;
        let start = usize::try_from(after).map_err(|_| ApiError::bad("invalid_reason_cursor"))?;
        if start > facts.reasons.len() {
            return Err(ApiError::domain("invalid_reason_cursor"));
        }
        let total = facts.reasons.len() as u64;
        let reasons: Vec<_> = facts.reasons.into_iter().skip(start).take(limit).collect();
        Ok(ReasonPageV1 {
            generation: self.info().provider_instance_id,
            start: after,
            next: after + reasons.len() as u64,
            total,
            reasons,
        })
    }
    pub(crate) fn skill_requests(&self, origin: AccountId32) -> ApiResult<Vec<SkillRequestCore>> {
        let root = self.info().root_account;
        Ok(self
            .facts()?
            .skill_requests
            .into_values()
            .filter(|request| origin == root || request.requester == origin)
            .collect())
    }
    pub(crate) fn dispute(&self, id: EntityId) -> ApiResult<CoreDisputeView> {
        let mut facts = self.facts()?;
        let dispute = facts
            .disputes
            .remove(&id)
            .ok_or_else(|| ApiError::domain("dispute_not_found"))?;
        let milestone = facts
            .projects
            .remove(&dispute.project_id)
            .and_then(|project| {
                project
                    .proposals
                    .into_iter()
                    .flat_map(|proposal| proposal.milestones)
                    .find(|milestone| milestone.milestone_id == dispute.milestone_id)
            })
            .ok_or_else(ApiError::unavailable)?;
        Ok(CoreDisputeView { dispute, milestone })
    }
    pub(crate) fn bramp_deposit(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> ApiResult<DepositView> {
        let mut ext = self
            .stored
            .checkpoint
            .clone()
            .restore()
            .map_err(|_| ApiError::unavailable())?;
        let deposit = facts::deposit(&mut ext, &self.state, id)
            .map_err(|_| ApiError::unavailable())?
            .ok_or_else(|| ApiError::domain("deposit_not_found"))?;
        if deposit.deposit_id != id {
            return Err(ApiError::unavailable());
        }
        if origin != deposit.owner && origin != self.info().root_account {
            return Err(ApiError::domain("deposit_owner_or_system_required"));
        }
        Ok(deposit)
    }
    pub(crate) fn bramp_withdrawal(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> ApiResult<WithdrawalView> {
        let mut ext = self
            .stored
            .checkpoint
            .clone()
            .restore()
            .map_err(|_| ApiError::unavailable())?;
        let withdrawal = facts::withdrawal(&mut ext, &self.state, id)
            .map_err(|_| ApiError::unavailable())?
            .ok_or_else(|| ApiError::domain("withdrawal_not_found"))?;
        if withdrawal.withdrawal_id != id {
            return Err(ApiError::unavailable());
        }
        if origin != withdrawal.owner && origin != self.info().root_account {
            return Err(ApiError::domain("withdrawal_owner_or_system_required"));
        }
        Ok(withdrawal)
    }
}
