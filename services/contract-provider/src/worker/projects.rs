//! ABI routing only. Project contracts own authorization and transitions.
use super::*;
use crate::transport::execution::PreparedExecution;
use parity_scale_codec::DecodeAll;
use protocol::EvidenceRecordReferenceV1 as Reference;
use protocol::ProviderCommand;
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    data.extend(args.encode());
    data
}
// These variants are contract inputs, separate from stored review records.
#[derive(Encode)]
enum ReviewReason<'a> {
    Evidence(Reference),
    Comment(&'a protocol::EntityId),
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
        } => {
            if request.deliverable.is_some() {
                return Err(Error::IncompleteCoverage);
            }
            input(
                "submit_milestone",
                (
                    milestone_id,
                    Option::<Reference>::None,
                    Option::<Vec<protocol::WorkerRating>>::None,
                    seed,
                ),
            )
        }
        ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
            milestone_id,
            worker_ratings,
            ..
        } => input(
            "submit_milestone",
            (
                milestone_id,
                Option::<Reference>::None,
                Some(worker_ratings),
                seed,
            ),
        ),
        ProviderCommand::AcceptMilestoneDelivery {
            milestone_id,
            submission_id,
            ..
        } => input(
            "accept_milestone",
            (
                milestone_id,
                submission_id,
                Option::<(u8, protocol::TeamRating)>::None,
            ),
        ),
        ProviderCommand::AcceptMilestoneCompletion {
            milestone_id,
            request,
            ..
        } => input(
            "accept_milestone",
            (
                milestone_id,
                request.submission_id,
                Some((request.coordinator_score, &request.team_rating)),
            ),
        ),
        ProviderCommand::RejectMilestoneDelivery {
            milestone_id,
            submission_id,
            comment_id,
            ..
        } => input(
            "reject_milestone",
            (
                milestone_id,
                submission_id,
                ReviewReason::Comment(comment_id),
            ),
        ),
        ProviderCommand::EvaluateProject { request, .. } => {
            input("evaluate_project", &request.ratings)
        }
        ProviderCommand::OpenDisputeWithComment(request) => input(
            "open_dispute",
            (
                request.milestone_id,
                request.rejected_submission_id,
                ReviewReason::Comment(&request.comment_id),
                seed,
                event_cursor,
            ),
        ),
        ProviderCommand::SubmitMilestoneCommitted {
            milestone_id,
            evidence,
            worker_ratings,
            ..
        } => input(
            "submit_milestone",
            (
                milestone_id,
                Some(Reference::Committed(evidence.clone())),
                worker_ratings,
                seed,
            ),
        ),
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
                ReviewReason::Evidence(Reference::Committed(evidence.clone())),
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
                ReviewReason::Evidence(Reference::Committed(evidence.clone())),
                seed,
                event_cursor,
            ),
        ),
        ProviderCommand::RespondDisputeCommitted {
            dispute_id,
            evidence,
            ..
        } => input(
            "respond_to_dispute",
            (dispute_id, Reference::Committed(evidence.clone())),
        ),
        ProviderCommand::CreateProjectCore(content) => input("create_project", (content, seed)),
        ProviderCommand::SetCompensationPolicy(policy) => input("set_compensation_policy", policy),
        ProviderCommand::CreateProposalCore {
            proposal, content, ..
        } => input("create_proposal", (proposal, content, seed)),
        ProviderCommand::UpdateProposalCore {
            proposal_id,
            proposal,
            content,
            ..
        } => input("update_proposal", (proposal_id, proposal, content, seed)),
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
        } => input("request_proposal_changes", (proposal_id, content)),
        ProviderCommand::WithdrawProposal {
            proposal_id,
            expected_revision,
            ..
        } => input("withdraw_proposal", (proposal_id, expected_revision)),
        ProviderCommand::AcceptPlanningDelivery {
            expected_revision, ..
        } => input("accept_planning_delivery", expected_revision),
        // Unsupported API operations need only a real read of current Project state.
        ProviderCommand::QuotePlanning { .. } | ProviderCommand::AcceptPlanningQuote { .. } => {
            input("view", ())
        }
        ProviderCommand::CreateTaskCore {
            task_storage_id,
            assignees,
            ..
        } => input("create_task", (task_storage_id, assignees)),
        ProviderCommand::SetTaskAssigneesCore {
            task_storage_id,
            task_id,
            assignees,
            ..
        } => input("set_task_assignees", (task_storage_id, task_id, assignees)),
        ProviderCommand::DeleteTask {
            task_storage_id,
            task_id,
            expected_revision,
            ..
        } => input("delete_task", (task_storage_id, task_id, expected_revision)),
        ProviderCommand::CancelProjectCore { content, .. } => input("cancel_project", content),
        ProviderCommand::DisputePlanningCore { content, .. } => input("dispute_planning", content),
        _ => return Err(Error::IncompleteCoverage),
    })
}

/// Preserve the API rejection of unsupported planning-price commands. The saved
/// outcome is the actual metered view call, never invented contract return bytes.
pub(super) fn unsupported_planning(
    command: &ProviderCommand,
    outcome: &Outcome,
) -> Result<Option<&'static str>, Error> {
    if !matches!(
        command,
        ProviderCommand::QuotePlanning { .. } | ProviderCommand::AcceptPlanningQuote { .. }
    ) || outcome.status != Status::Success
    {
        return Ok(None);
    }
    let project =
        Result::<protocol::provider_core::ProjectCore, u8>::decode_all(&mut &outcome.data[..])
            .map_err(|_| Error::InvalidState)?
            .map_err(|_| Error::InvalidState)?;
    Ok(Some(if project.cancelled {
        "project_cancelled"
    } else if project.active_dispute_id.is_some() {
        "project_disputed"
    } else {
        "separate_planning_price_removed"
    }))
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
