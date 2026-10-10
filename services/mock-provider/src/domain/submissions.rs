use std::collections::BTreeSet;

use generated_contracts::{
    AccountId32, CompletionSubmission, EntityId, EvidenceReference, EvidenceRequest,
    MilestoneStatus, MilestoneView, ProjectView, SubmissionReview, UnixSeconds, WorkerRating,
};

use super::project::milestone_mut;
use crate::{Error, Result, random_id, require};

pub(super) fn submit(
    project: &mut ProjectView,
    origin: AccountId32,
    milestone_id: EntityId,
    deliverable: Option<EvidenceReference>,
    worker_ratings: Option<&[WorkerRating]>,
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
    let legacy_ratings = worker_ratings.is_some();
    let worker_ratings = worker_ratings.unwrap_or_default();
    let assigned: BTreeSet<_> = milestone.assignments.iter().map(|a| a.worker).collect();
    let rated: BTreeSet<_> = worker_ratings.iter().map(|r| r.worker).collect();
    require(
        !legacy_ratings || (assigned == rated && rated.len() == worker_ratings.len()),
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
        deliverable,
        submitted_by: origin,
        submitted_at: now,
        worker_ratings: worker_ratings.to_vec(),
        review: SubmissionReview::PendingReview,
    });
    milestone.worker_ratings = worker_ratings.to_vec();
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
