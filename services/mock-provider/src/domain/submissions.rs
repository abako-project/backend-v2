use std::collections::BTreeSet;

use generated_contracts::{
    AccountId32, CompletionSubmission, EntityId, EvidenceRequest, MilestoneStatus, MilestoneView,
    ProjectView, RequestMilestoneCompletionRequest, SubmissionReview, UnixSeconds,
};

use super::project::milestone_mut;
use crate::{Error, Result, random_id, require};

pub(super) fn submit(
    project: &mut ProjectView,
    origin: AccountId32,
    milestone_id: EntityId,
    request: &RequestMilestoneCompletionRequest,
    now: UnixSeconds,
) -> Result<EntityId> {
    require(project.coordinator == origin, "coordinator_required")?;
    let milestone = milestone_mut(project, milestone_id)?;
    require(
        matches!(
            milestone.status,
            Some(MilestoneStatus::InProgress | MilestoneStatus::ChangesRequested)
        ) && !milestone.frozen,
        "invalid_milestone_state",
    )?;
    let assigned: BTreeSet<_> = milestone.assignments.iter().map(|a| a.worker).collect();
    let rated: BTreeSet<_> = request.worker_ratings.iter().map(|r| r.worker).collect();
    require(
        assigned == rated && rated.len() == request.worker_ratings.len(),
        "worker_ratings_mismatch",
    )?;
    let version = u64::try_from(milestone.submissions.len())
        .map_err(|_| Error::internal())?
        .checked_add(1)
        .ok_or_else(Error::internal)?;
    let submission_id = random_id()?;
    milestone.submissions.push(CompletionSubmission {
        submission_id,
        version,
        deliverable: request.deliverable.clone(),
        submitted_by: origin,
        submitted_at: now,
        worker_ratings: request.worker_ratings.clone(),
        review: SubmissionReview::PendingReview,
    });
    milestone.worker_ratings.clone_from(&request.worker_ratings);
    milestone.status = Some(MilestoneStatus::CompletionRequested);
    Ok(submission_id)
}

pub(super) fn pending(
    milestone: &mut MilestoneView,
    submission_id: EntityId,
) -> Result<&mut CompletionSubmission> {
    let submission = milestone
        .submissions
        .last_mut()
        .ok_or_else(|| Error::domain("submission_not_current"))?;
    require(
        submission.submission_id == submission_id,
        "submission_not_current",
    )?;
    require(
        submission.review == SubmissionReview::PendingReview,
        "submission_not_pending",
    )?;
    Ok(submission)
}

pub(super) fn reject(
    project: &mut ProjectView,
    origin: AccountId32,
    milestone_id: EntityId,
    submission_id: EntityId,
    request: &EvidenceRequest,
    now: UnixSeconds,
) -> Result<()> {
    require(project.client == origin, "client_required")?;
    let milestone = milestone_mut(project, milestone_id)?;
    require(
        milestone.status == Some(MilestoneStatus::CompletionRequested) && !milestone.frozen,
        "invalid_milestone_state",
    )?;
    pending(milestone, submission_id)?.review = SubmissionReview::Rejected {
        reason: request.evidence.clone(),
        reviewed_by: origin,
        reviewed_at: now,
    };
    milestone.status = Some(MilestoneStatus::ChangesRequested);
    Ok(())
}
