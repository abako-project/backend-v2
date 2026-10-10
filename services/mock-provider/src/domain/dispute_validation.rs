use super::State;
use crate::{Error, Result, require};
use generated_contracts::{
    CompletionSubmission, Dispute, EntityId, MilestoneStatus, MilestoneView, ProjectView,
    SubmissionReview,
};
use std::collections::BTreeSet;

impl State {
    pub(super) fn validate_disputes(&self, identities: &mut BTreeSet<EntityId>) -> Result<()> {
        for project in self.projects.values() {
            for milestone in project.proposals.iter().flat_map(|p| &p.milestones) {
                validate_milestone(self, project, milestone, identities)?;
            }
            if let Some(id) = project.active_dispute_id {
                require(
                    self.disputes
                        .get(&id)
                        .is_some_and(|case| case.project_id == project.project_id),
                    "invalid_dispute_link",
                )?;
            }
        }
        for (id, case) in &self.disputes {
            validate_case(self, *id, case, identities)?;
        }
        Ok(())
    }
}

fn validate_milestone(
    state: &State,
    project: &ProjectView,
    milestone: &MilestoneView,
    identities: &mut BTreeSet<EntityId>,
) -> Result<()> {
    for (index, submission) in milestone.submissions.iter().enumerate() {
        validate_submission(
            project,
            submission,
            index,
            milestone.submissions.len(),
            identities,
        )?;
    }
    let current = milestone.submissions.last();
    require(
        matches!(
            (
                milestone.status,
                current.map(|submission| &submission.review)
            ),
            (
                None | Some(MilestoneStatus::NotStarted | MilestoneStatus::InProgress),
                None
            ) | (
                Some(MilestoneStatus::CompletionRequested),
                Some(SubmissionReview::PendingReview)
            ) | (
                Some(MilestoneStatus::ChangesRequested | MilestoneStatus::Disputed),
                Some(
                    SubmissionReview::Rejected { .. }
                        | SubmissionReview::RejectedWithComment { .. }
                )
            ) | (
                Some(MilestoneStatus::Completed),
                Some(SubmissionReview::Accepted { .. })
            )
        ),
        "invalid_submission_state",
    )?;
    if let Some(current) = current {
        require(
            current.worker_ratings == milestone.worker_ratings,
            "invalid_submission_ratings",
        )?;
    }
    if milestone.status == Some(MilestoneStatus::Disputed) {
        require(
            project
                .active_dispute_id
                .and_then(|id| state.disputes.get(&id))
                .is_some_and(|case| case.milestone_id == milestone.milestone_id)
                && milestone.frozen,
            "invalid_dispute_state",
        )?;
    }
    Ok(())
}

fn validate_submission(
    project: &ProjectView,
    submission: &CompletionSubmission,
    index: usize,
    count: usize,
    identities: &mut BTreeSet<EntityId>,
) -> Result<()> {
    require(
        identities.insert(submission.submission_id)
            && submission.version == u64::try_from(index).map_err(|_| Error::internal())? + 1
            && submission.submitted_by == project.coordinator,
        "invalid_submission_state",
    )?;
    match &submission.review {
        SubmissionReview::PendingReview => {}
        SubmissionReview::Rejected {
            reviewed_by,
            reviewed_at,
            ..
        }
        | SubmissionReview::RejectedWithComment {
            reviewed_by,
            reviewed_at,
            ..
        }
        | SubmissionReview::Accepted {
            reviewed_by,
            reviewed_at,
        } => require(
            *reviewed_by == project.client && *reviewed_at >= submission.submitted_at,
            "invalid_review_state",
        )?,
    }
    if index + 1 < count {
        require(
            matches!(
                submission.review,
                SubmissionReview::Rejected { .. } | SubmissionReview::RejectedWithComment { .. }
            ),
            "invalid_submission_history",
        )?;
    }
    Ok(())
}

fn validate_case(
    state: &State,
    id: EntityId,
    case: &Dispute,
    identities: &mut BTreeSet<EntityId>,
) -> Result<()> {
    require(
        id == case.dispute_id && identities.insert(id),
        "invalid_dispute_id",
    )?;
    let project = state
        .projects
        .get(&case.project_id)
        .ok_or_else(Error::internal)?;
    let proposal = project
        .proposals
        .iter()
        .find(|proposal| {
            proposal
                .milestones
                .iter()
                .any(|milestone| milestone.milestone_id == case.milestone_id)
        })
        .ok_or_else(Error::internal)?;
    let milestone = proposal
        .milestones
        .iter()
        .find(|milestone| milestone.milestone_id == case.milestone_id)
        .ok_or_else(Error::internal)?;
    require(
        !project.cancelled
            && project.active_dispute_id == Some(id)
            && proposal.revision == case.proposal_revision
            && milestone.status == Some(MilestoneStatus::Disputed)
            && milestone
                .submissions
                .last()
                .is_some_and(|submission| submission.submission_id == case.rejected_submission_id)
            && case.context_event_cursor
                <= u64::try_from(state.events.len()).map_err(|_| Error::internal())?,
        "invalid_dispute_state",
    )?;
    require(
        (case.opened_by == project.client && case.counterparty == project.coordinator)
            || (case.opened_by == project.coordinator && case.counterparty == project.client),
        "invalid_dispute_parties",
    )?;
    require(
        case.evidence.is_some() != case.opening_comment_id.is_some(),
        "invalid_dispute_opening",
    )?;
    if let Some(response) = &case.response {
        require(
            response.author == case.counterparty && response.occurred_at >= case.opened_at,
            "invalid_dispute_response",
        )?;
    }
    Ok(())
}
