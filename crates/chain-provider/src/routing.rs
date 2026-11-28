//! ABI routing, ported from the embedded provider. Domain permissions and
//! rules run only in the pinned contracts.
use crate::transport::execution::{Effect, ExecutionInputs};
use crate::{Account, Error};
use kreivo::message as input;
use parity_scale_codec::{Decode, DecodeAll, Encode};
use protocol::EvidenceRecordReferenceV1 as Reference;
use protocol::{DomainEventKind, EntityId, ProviderCommand};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::collections::BTreeSet;

#[derive(Decode)]
struct ContractEffect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<Account>,
    allocated_task_id: Option<u32>,
}

/// Where a command goes.
pub(crate) enum Target {
    /// A message of the Dao.
    Dao(Vec<u8>),
    /// A message of the Project with this ID, or of the Dao when `None`.
    Project(Option<EntityId>, Vec<u8>),
}

/// The contract message of `command`.
pub(crate) fn route(
    command: &ProviderCommand,
    inputs: &ExecutionInputs,
    event_cursor: u64,
) -> Result<Target, Error> {
    match dao_input(command, inputs) {
        Ok(data) => Ok(Target::Dao(data)),
        Err(Error::IncompleteCoverage) => {
            let data = project_input(command, inputs.seed, event_cursor)?;
            // The user calls Project directly. Dao never becomes the payer
            // for asset deposits.
            if matches!(
                command,
                ProviderCommand::CreateProjectCore(_) | ProviderCommand::SetCompensationPolicy(_)
            ) {
                Ok(Target::Project(None, data))
            } else {
                let id = command.project_id().ok_or(Error::IncompleteCoverage)?;
                Ok(Target::Project(Some(id), data))
            }
        }
        Err(error) => Err(error),
    }
}

/// The input that asks the Dao for a Project's address.
pub(crate) fn resolve_project(id: EntityId) -> Vec<u8> {
    input("resolve_project", id)
}

/// The address `resolve_project` returned, if the project exists.
pub(crate) fn resolved(data: &[u8]) -> Result<Account, Error> {
    Result::<Result<Account, String>, u8>::decode_all(&mut &data[..])
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)
}

fn dao_input(command: &ProviderCommand, inputs: &ExecutionInputs) -> Result<Vec<u8>, Error> {
    // Dao commands allocate at most one ID, before any random selection. This is
    // the same first 16 bytes of the version-1 locked native StdRng stream.
    let id = || {
        let mut bytes = [0; 16];
        StdRng::from_seed(inputs.seed).fill_bytes(&mut bytes);
        EntityId::from_bytes(bytes)
    };
    Ok(match command {
        ProviderCommand::FundAccount(request) => {
            input("fund_account", (request.account, request.amount))
        }
        ProviderCommand::CreateDeposit(request) => {
            input("create_deposit", (request.amount, inputs.seed))
        }
        ProviderCommand::ConfirmDeposit { deposit_id } => input("confirm_deposit", deposit_id),
        ProviderCommand::CreateWithdrawal(request) => {
            input("create_withdrawal", (request.amount, inputs.seed))
        }
        ProviderCommand::CancelWithdrawal { withdrawal_id } => {
            input("cancel_withdrawal", withdrawal_id)
        }

        ProviderCommand::RegisterWorkerCore(request) => input("register_worker", (request, id())),
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetScorePolicy(policy) => input("set_score_policy", policy),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::UpsertCatalogEntryCore { kind, id, content } => {
            input("upsert_catalog_entry", (kind, id, content))
        }
        ProviderCommand::DeleteCatalogEntry(request) => input("delete_catalog_entry", request),
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            input("create_skill_request", (role_ids, content, id()))
        }
        ProviderCommand::DecideSkillRequest(request) => input("decide_skill_request", request),
        _ => return Err(Error::IncompleteCoverage),
    })
}
// These variants are contract inputs, separate from stored review records.
#[derive(Encode)]
enum ReviewReason<'a> {
    Evidence(Reference),
    Comment(&'a protocol::EntityId),
}
fn project_input(
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

/// What a message's return value means for the shell: the effect of a
/// success, or the code of a known domain rejection.
pub(crate) fn transition(
    reverted: bool,
    data: &[u8],
) -> Result<Result<Effect, &'static str>, Error> {
    let decoded = Result::<Result<ContractEffect, String>, u8>::decode_all(&mut &data[..])
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)?;
    match (reverted, decoded) {
        (false, Ok(effect)) => {
            let recipients: BTreeSet<_> = effect.recipients.iter().copied().collect();
            if recipients.iter().copied().collect::<Vec<_>>() != effect.recipients {
                return Err(Error::InvalidState);
            }
            Ok(Ok(Effect {
                kind: effect.kind,
                entity_id: effect.entity_id,
                recipients,
                allocated_task_id: effect.allocated_task_id,
            }))
        }
        (true, Err(code)) => Ok(Err(domain_error(&code)?)),
        _ => Err(Error::InvalidState),
    }
}

/// Preserve the API rejection of unsupported planning-price commands, from a
/// real read of the Project's state.
pub(crate) fn unsupported_planning(
    command: &ProviderCommand,
    reverted: bool,
    data: &[u8],
) -> Result<Option<&'static str>, Error> {
    if !matches!(
        command,
        ProviderCommand::QuotePlanning { .. } | ProviderCommand::AcceptPlanningQuote { .. }
    ) || reverted
    {
        return Ok(None);
    }
    let project = Result::<protocol::provider_core::ProjectCore, u8>::decode_all(&mut &data[..])
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

fn domain_error(code: &str) -> Result<&'static str, Error> {
    // Only known public domain rejections can consume a nonce. In particular,
    // child-instantiation failure and invalid_worker_state are infrastructure /
    // invariant failures in the native engine, not accepted failed operations.
    match code {
        "invalid_contract" => Ok("invalid_contract"),
        "worker_exists" => Ok("worker_exists"),
        "worker_not_found" => Ok("worker_not_found"),
        "unknown_qualification" => Ok("unknown_qualification"),
        "coordinator_not_eligible" => Ok("coordinator_not_eligible"),
        "system_origin_required" => Ok("system_origin_required"),
        "fixed_coordinator_role" => Ok("fixed_coordinator_role"),
        "catalog_entry_in_use" => Ok("catalog_entry_in_use"),
        "catalog_entry_not_found" => Ok("catalog_entry_not_found"),
        "skill_name_exists" => Ok("skill_name_exists"),
        "unknown_role" => Ok("unknown_role"),
        "request_id_collision" => Ok("request_id_collision"),
        "skill_request_not_found" => Ok("skill_request_not_found"),
        "skill_request_decided" => Ok("skill_request_decided"),
        "catalog_id_exhausted" => Ok("catalog_id_exhausted"),
        "calendar_overcommitted" => Ok("calendar_overcommitted"),
        "calendar_too_large" => Ok("calendar_too_large"),
        "insufficient_capacity" => Ok("insufficient_capacity"),
        "arithmetic_or_calendar_error" => Ok("arithmetic_or_calendar_error"),
        "project_not_found" => Ok("project_not_found"),
        "invalid_time" => Ok("invalid_time"),
        "coordinator_required" => Ok("coordinator_required"),
        "client_required" => Ok("client_required"),
        "planning_not_accepted" => Ok("planning_not_accepted"),
        "proposal_not_found" => Ok("proposal_not_found"),
        "proposal_not_draft" => Ok("proposal_not_draft"),
        "proposal_not_pending" => Ok("proposal_not_pending"),
        "proposal_too_large" => Ok("proposal_too_large"),
        "storage_entry_too_large" => Ok("storage_entry_too_large"),
        "revision_conflict" => Ok("revision_conflict"),
        "revision_exhausted" => Ok("revision_exhausted"),
        "execution_already_approved" => Ok("execution_already_approved"),
        "invalid_planning_state" => Ok("invalid_planning_state"),
        "separate_planning_price_removed" => Ok("separate_planning_price_removed"),
        "task_storage_not_found" => Ok("task_storage_not_found"),
        "task_not_found" => Ok("task_not_found"),
        "unknown_task_assignee" => Ok("unknown_task_assignee"),
        "task_keys_exhausted" => Ok("task_keys_exhausted"),
        "task_assignee_required" => Ok("task_assignee_required"),
        "client_read_only" => Ok("client_read_only"),
        "project_party_required" => Ok("project_party_required"),
        "project_completed" => Ok("project_completed"),
        "project_cancelled" => Ok("project_cancelled"),
        "project_disputed" => Ok("project_disputed"),
        "delivery_date_before_milestones" => Ok("delivery_date_before_milestones"),
        "invalid_delivery_date" => Ok("invalid_delivery_date"),
        "invalid_delivery_schedule" => Ok("invalid_delivery_schedule"),
        "empty_milestones" => Ok("empty_milestones"),
        "no_available_candidate" => Ok("no_available_candidate"),
        "invalid_project_state" => Err(Error::Invariant("invalid_project_state")),
        "duplicate_entity" => Err(Error::Invariant("duplicate_entity")),
        "bramp_id_collision" => Ok("duplicate_entity"),
        "insufficient_balance" => Ok("insufficient_balance"),
        "mock_funding_disabled" => Ok("mock_funding_disabled"),
        "deposit_not_found" => Ok("deposit_not_found"),
        "deposit_already_confirmed" => Ok("deposit_already_confirmed"),
        "withdrawal_not_found" => Ok("withdrawal_not_found"),
        "withdrawal_owner_or_system_required" => Ok("withdrawal_owner_or_system_required"),
        "withdrawal_already_cancelled" => Ok("withdrawal_already_cancelled"),
        "invalid_deposit_state" => Err(Error::Invariant("invalid_deposit_state")),
        "invalid_withdrawal_state" => Err(Error::Invariant("invalid_withdrawal_state")),
        "asset_transfer_failed" => Err(Error::Invariant("asset_transfer_failed")),
        "invalid_asset_state" => Err(Error::Invariant("invalid_asset_state")),
        "active_dispute_exists" => Ok("active_dispute_exists"),
        "milestone_not_changes_requested" => Ok("milestone_not_changes_requested"),
        "submission_not_current" => Ok("submission_not_current"),
        "submission_not_pending" => Ok("submission_not_pending"),
        "submission_not_rejected" => Ok("submission_not_rejected"),
        "milestone_not_found" => Ok("milestone_not_found"),
        "invalid_milestone_state" => Ok("invalid_milestone_state"),
        "worker_ratings_mismatch" => Ok("worker_ratings_mismatch"),
        "project_not_completed" => Ok("project_not_completed"),
        "project_already_evaluated" => Ok("project_already_evaluated"),
        "project_participant_required" => Ok("project_participant_required"),
        "evaluation_participants_mismatch" => Ok("evaluation_participants_mismatch"),
        "dispute_not_found" => Ok("dispute_not_found"),
        "invalid_dispute_target" => Ok("invalid_dispute_target"),
        "dispute_response_forbidden" => Ok("dispute_response_forbidden"),
        "dispute_already_answered" => Ok("dispute_already_answered"),
        "score_overflow" => Ok("score_overflow"),
        "invalid_review_state" => Err(Error::Invariant("invalid_review_state")),
        "invalid_dispute_response" => Err(Error::Invariant("invalid_dispute_response")),
        "invalid_submission_state" => Err(Error::Invariant("invalid_submission_state")),
        "invalid_dispute_state" => Err(Error::Invariant("invalid_dispute_state")),
        "invalid_dispute_id" => Err(Error::Invariant("invalid_dispute_id")),
        "invalid_assignment_state" => Err(Error::Invariant("invalid_assignment_state")),
        "provider_unavailable" => Err(Error::Invariant("provider_unavailable")),
        "invalid_storage_state" => Err(Error::Invariant("invalid_storage_state")),
        "project_instantiation_failed" => Err(Error::Invariant("project_instantiation_failed")),
        "project_code_missing" => Err(Error::Invariant("project_code_missing")),
        "registered_project_required" => Err(Error::Invariant("registered_project_required")),
        "incomplete_money_coverage" => Err(Error::IncompleteCoverage),
        "invalid_worker_state" => Err(Error::Invariant("invalid_worker_state")),
        "invalid_catalog_state" => Err(Error::Invariant("invalid_catalog_state")),
        "invalid_calendar_state" => Err(Error::Invariant("invalid_calendar_state")),
        _ => Err(Error::InvalidState),
    }
}
