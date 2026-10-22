//! M5 ABI routing only. Project contracts own authorization and transitions.
use super::*;
use generated_contracts::{ProviderCommand, UnsignedContractCallV1};
use parity_scale_codec::DecodeAll;
use provider_execution::PreparedExecution;
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    data.extend(args.encode());
    data
}
fn command_input(call: &UnsignedContractCallV1, seed: [u8; 32]) -> Result<Vec<u8>, Error> {
    Ok(match &call.command {
        ProviderCommand::CreateProject(request) => input(
            "create_project",
            (&request.title, &request.description, seed),
        ),
        ProviderCommand::SetCompensationPolicy(policy) => input("set_compensation_policy", policy),
        ProviderCommand::CreateProposal { proposal, .. } => {
            input("create_proposal", (proposal, seed))
        }
        ProviderCommand::UpdateProposal {
            proposal_id,
            proposal,
            ..
        } => input("update_proposal", (proposal_id, proposal, seed)),
        ProviderCommand::DeleteProposal { proposal_id, .. } => {
            input("delete_proposal", proposal_id)
        }
        ProviderCommand::SubmitProposal { proposal_id, .. } => {
            input("submit_proposal", proposal_id)
        }
        ProviderCommand::SetProposalDelivery {
            proposal_id,
            request,
            ..
        } => input("set_proposal_delivery", (proposal_id, request)),
        ProviderCommand::RequestProposalChanges {
            proposal_id,
            request,
            ..
        } => input("request_proposal_changes", (proposal_id, request)),
        ProviderCommand::WithdrawProposal {
            proposal_id,
            expected_revision,
            ..
        } => input("withdraw_proposal", (proposal_id, expected_revision)),
        ProviderCommand::AcceptPlanningDelivery {
            expected_revision, ..
        } => input("accept_planning_delivery", expected_revision),
        ProviderCommand::QuotePlanning { .. } | ProviderCommand::AcceptPlanningQuote { .. } => {
            input("removed_planning_price", ())
        }
        ProviderCommand::CreateTask {
            task_storage_id,
            task,
            ..
        } => input("create_task", (task_storage_id, task)),
        ProviderCommand::EditTask {
            task_storage_id,
            task_id,
            task,
            ..
        } => input("edit_task", (task_storage_id, task_id, task)),
        ProviderCommand::UpdateTaskProgress {
            task_storage_id,
            task_id,
            progress,
            ..
        } => input("update_task_progress", (task_storage_id, task_id, progress)),
        ProviderCommand::DeleteTask {
            task_storage_id,
            task_id,
            expected_revision,
            ..
        } => input("delete_task", (task_storage_id, task_id, expected_revision)),
        ProviderCommand::CancelProject { request, .. } => input("cancel_project", request),
        ProviderCommand::DisputePlanning { request, .. } => input("dispute_planning", request),
        _ => return Err(Error::IncompleteCoverage),
    })
}
pub(super) fn execute(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    directory: &Account,
    pending: &PreparedExecution,
    gas: Weight,
    time: u64,
) -> Result<Outcome, Error> {
    let call = pending.call();
    let data = command_input(call, pending.inputs().seed)?;
    let mut invocation = Invocation {
        caller: Account::new(call.origin.into_bytes()),
        action: Action::Call {
            address: directory.clone(),
        },
        input: data,
        gas,
        timestamp_ms: time,
    };
    invocation.validate()?;
    let mut resolution_gas = Weight::zero();
    if !matches!(
        call.command,
        ProviderCommand::CreateProject(_) | ProviderCommand::SetCompensationPolicy(_)
    ) {
        let id = call.command.project_id().ok_or(Error::IncompleteCoverage)?;
        let resolved = run(
            ext,
            bundles,
            Invocation {
                caller: invocation.caller.clone(),
                action: Action::Call {
                    address: directory.clone(),
                },
                input: input("resolve_project", id),
                gas,
                timestamp_ms: time,
            },
        )?;
        if resolved.status != Status::Success {
            return Ok(resolved);
        }
        let address = Result::<Result<Account, String>, u8>::decode_all(&mut &resolved.data[..])
            .map_err(|_| Error::InvalidState)?
            .map_err(|_| Error::InvalidState)?
            .map_err(|_| Error::InvalidState)?;
        resolution_gas = resolved.gas_consumed;
        invocation.gas = gas
            .checked_sub(&resolution_gas)
            .ok_or(Error::InvalidState)?;
        invocation.action = Action::Call { address };
    }
    // The user calls Project directly. Directory/factory never becomes the
    // payer when M6 adds asset deposits. Lookup and execution share one gas cap.
    let mut result = run(ext, bundles, invocation)?;
    result.gas_consumed = result.gas_consumed.saturating_add(resolution_gas);
    Ok(result)
}
