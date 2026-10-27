//! ABI routing only. Project contracts own authorization and transitions.
use super::*;
use parity_scale_codec::DecodeAll;
use protocol::EvidenceRecordReferenceV1 as Reference;
use protocol::ProviderCommand;
use provider::execution::PreparedExecution;
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    data.extend(args.encode());
    data
}
fn command_input(
    command: &ProviderCommand,
    seed: [u8; 32],
    event_cursor: u64,
) -> Result<Vec<u8>, Error> {
    Ok(match command {
        ProviderCommand::ApproveExecution {
            proposal_id,
            expected_revision,
            ..
        } => input("approve_execution", (proposal_id, expected_revision, seed)),
        ProviderCommand::SubmitMilestoneDelivery {
            milestone_id,
            request,
            ..
        } => input(
            "submit_milestone",
            (
                milestone_id,
                if request.deliverable.is_none() {
                    None::<Reference>
                } else {
                    return Err(Error::IncompleteCoverage);
                },
                seed,
            ),
        ),
        ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
            milestone_id,
            worker_ratings,
            ..
        } => input(
            "submit_rated_milestone",
            (
                milestone_id,
                Option::<Reference>::None,
                worker_ratings,
                seed,
            ),
        ),
        ProviderCommand::AcceptMilestoneDelivery {
            milestone_id,
            submission_id,
            ..
        } => input("accept_milestone", (milestone_id, submission_id)),
        ProviderCommand::AcceptMilestoneCompletion {
            milestone_id,
            request,
            ..
        } => input(
            "accept_rated_milestone",
            (
                milestone_id,
                request.submission_id,
                request.coordinator_score,
                &request.team_rating,
            ),
        ),
        ProviderCommand::RejectMilestoneDelivery {
            milestone_id,
            submission_id,
            comment_id,
            ..
        } => input(
            "reject_milestone_comment",
            (milestone_id, submission_id, comment_id),
        ),
        ProviderCommand::EvaluateProject { request, .. } => {
            input("evaluate_project", &request.ratings)
        }
        ProviderCommand::OpenDisputeWithComment(request) => input(
            "open_dispute_comment",
            (
                request.milestone_id,
                request.rejected_submission_id,
                request.comment_id,
                seed,
                event_cursor,
            ),
        ),
        ProviderCommand::SubmitMilestoneCommitted {
            milestone_id,
            evidence,
            worker_ratings,
            ..
        } => {
            let reference = Some(Reference::Committed(evidence.clone()));
            match worker_ratings {
                Some(ratings) => input(
                    "submit_rated_milestone",
                    (milestone_id, reference, ratings, seed),
                ),
                None => input("submit_milestone", (milestone_id, reference, seed)),
            }
        }
        ProviderCommand::RejectMilestoneCommitted {
            milestone_id,
            submission_id,
            evidence,
            ..
        } => input(
            "reject_milestone",
            (
                milestone_id,
                submission_id,
                Reference::Committed(evidence.clone()),
            ),
        ),
        ProviderCommand::OpenDisputeCommitted {
            milestone_id,
            rejected_submission_id,
            evidence,
            ..
        } => input(
            "open_dispute",
            (
                milestone_id,
                rejected_submission_id,
                Reference::Committed(evidence.clone()),
                seed,
                event_cursor,
            ),
        ),
        ProviderCommand::RespondDisputeCommitted {
            dispute_id,
            evidence,
            ..
        } => input(
            "respond_dispute",
            (dispute_id, Reference::Committed(evidence.clone())),
        ),

        ProviderCommand::CreateProjectCore(content) => input("create_project", (content, seed)),
        ProviderCommand::SetCompensationPolicy(policy) => input("set_compensation_policy", policy),
        ProviderCommand::CreateProposalCore {
            proposal, content, ..
        } => input("create_proposal_core", (proposal, content, seed)),
        ProviderCommand::UpdateProposalCore {
            proposal_id,
            proposal,
            content,
            ..
        } => input(
            "update_proposal_core",
            (proposal_id, proposal, content, seed),
        ),
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
        ProviderCommand::RequestProposalChangesCore {
            proposal_id,
            content,
            ..
        } => input("request_proposal_changes_core", (proposal_id, content)),
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
        ProviderCommand::CreateTaskCore {
            task_storage_id,
            assignees,
            ..
        } => input("create_task_core", (task_storage_id, assignees)),
        ProviderCommand::SetTaskAssigneesCore {
            task_storage_id,
            task_id,
            assignees,
            ..
        } => input(
            "set_task_assignees_core",
            (task_storage_id, task_id, assignees),
        ),
        ProviderCommand::DeleteTask {
            task_storage_id,
            task_id,
            expected_revision,
            ..
        } => input("delete_task", (task_storage_id, task_id, expected_revision)),
        ProviderCommand::CancelProjectCore { content, .. } => input("cancel_project_core", content),
        ProviderCommand::DisputePlanningCore { content, .. } => {
            input("dispute_planning_core", content)
        }
        _ => return Err(Error::IncompleteCoverage),
    })
}
pub(super) fn execute(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &mut super::dao::State,
    pending: &PreparedExecution,
    gas: Weight,
    time: u64,
    event_cursor: u64,
) -> Result<Outcome, Error> {
    let dao = &state.dao;
    let call = pending.call();
    let command = &call.command;
    let data = command_input(command, pending.inputs().seed, event_cursor)?;
    let mut invocation = Invocation {
        caller: Account::new(call.origin.into_bytes()),
        action: Action::Call {
            address: dao.clone(),
        },
        input: data,
        gas,
        timestamp_ms: time,
    };
    invocation.validate_with_budget(crate::EXECUTION_GAS)?;
    let mut resolution_gas = Weight::zero();
    if !matches!(
        command,
        ProviderCommand::CreateProjectCore(_) | ProviderCommand::SetCompensationPolicy(_)
    ) {
        let id = call.command.project_id().ok_or(Error::IncompleteCoverage)?;
        let resolved = run(
            ext,
            bundles,
            Invocation {
                caller: invocation.caller.clone(),
                action: Action::Call {
                    address: dao.clone(),
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
    // The user calls Project directly. Dao/factory never becomes the
    // payer for asset deposits. Lookup and execution share one gas cap.
    let mut result = run(ext, bundles, invocation)?;
    result.gas_consumed = result.gas_consumed.saturating_add(resolution_gas);
    Ok(result)
}
