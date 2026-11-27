//! Read one committed revision. No native engine, replay or state comparison is involved.
use crate::transport::Error as ApiError;
use crate::{Error, State, facts};
use protocol::{provider_core::*, *};

type Result<T> = std::result::Result<T, Error>;

/// A refusal of the read itself, with its status.
fn refused(error: ApiError) -> Error {
    Error::Provider(error)
}
fn info(state: &State) -> ProviderInfo {
    state.shell.info.clone()
}
pub(crate) fn task_allocation(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    operation: OperationId,
) -> Result<Option<TaskAllocation>> {
    let allocation = state
        .shell
        .task_allocation(operation)
        .map_err(|error| refused(error.into()))?;
    if let Some(allocated) = &allocation {
        let next = facts::next_task_id(reader, state)?;
        if allocated.task_id >= next {
            return Err(refused(ApiError::unavailable()));
        }
    }
    Ok(allocation)
}
pub(crate) fn snapshot(reader: &mut kreivo::Reader<'_>, state: &State) -> Result<CoreSnapshot> {
    let facts = facts::read(reader, state)?;
    Ok(CoreSnapshot {
        schema_version: 5,
        info: info(state),
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
pub(crate) fn task_state(reader: &mut kreivo::Reader<'_>, state: &State) -> Result<TaskState> {
    let facts = facts::read(reader, state)?;
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
    let has_task_history = state.shell.receipts.values().any(|record| {
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
        generation: info(state).provider_instance_id,
        next_task_id: facts.next_task_id,
        has_task_history,
        storages,
    })
}
pub(crate) fn lifecycle_reasons(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    after: u64,
    limit: usize,
) -> Result<ReasonPageV1> {
    if !(1..=256).contains(&limit) {
        return Err(refused(ApiError::domain("invalid_reason_limit")));
    }
    let facts = facts::read(reader, state)?;
    let start =
        usize::try_from(after).map_err(|_| refused(ApiError::bad("invalid_reason_cursor")))?;
    if start > facts.reasons.len() {
        return Err(refused(ApiError::domain("invalid_reason_cursor")));
    }
    let total = facts.reasons.len() as u64;
    let reasons: Vec<_> = facts.reasons.into_iter().skip(start).take(limit).collect();
    Ok(ReasonPageV1 {
        generation: info(state).provider_instance_id,
        start: after,
        next: after + reasons.len() as u64,
        total,
        reasons,
    })
}
pub(crate) fn skill_requests(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    origin: AccountId32,
) -> Result<Vec<SkillRequestCore>> {
    let root = info(state).root_account;
    Ok(facts::read(reader, state)?
        .skill_requests
        .into_values()
        .filter(|request| origin == root || request.requester == origin)
        .collect())
}
pub(crate) fn dispute(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    id: EntityId,
) -> Result<CoreDisputeView> {
    let mut facts = facts::read(reader, state)?;
    let dispute = facts
        .disputes
        .remove(&id)
        .ok_or_else(|| refused(ApiError::domain("dispute_not_found")))?;
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
        .ok_or_else(|| refused(ApiError::unavailable()))?;
    Ok(CoreDisputeView { dispute, milestone })
}
pub(crate) fn bramp_deposit(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    origin: AccountId32,
    id: EntityId,
) -> Result<DepositView> {
    let deposit = facts::deposit(reader, state, id)?
        .ok_or_else(|| refused(ApiError::domain("deposit_not_found")))?;
    if deposit.deposit_id != id {
        return Err(refused(ApiError::unavailable()));
    }
    if origin != deposit.owner && origin != info(state).root_account {
        return Err(refused(ApiError::domain(
            "deposit_owner_or_system_required",
        )));
    }
    Ok(deposit)
}
pub(crate) fn bramp_withdrawal(
    reader: &mut kreivo::Reader<'_>,
    state: &State,
    origin: AccountId32,
    id: EntityId,
) -> Result<WithdrawalView> {
    let withdrawal = facts::withdrawal(reader, state, id)?
        .ok_or_else(|| refused(ApiError::domain("withdrawal_not_found")))?;
    if withdrawal.withdrawal_id != id {
        return Err(refused(ApiError::unavailable()));
    }
    if origin != withdrawal.owner && origin != info(state).root_account {
        return Err(refused(ApiError::domain(
            "withdrawal_owner_or_system_required",
        )));
    }
    Ok(withdrawal)
}
