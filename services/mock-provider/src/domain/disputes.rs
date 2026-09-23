use generated_contracts::{
    AccountId32, Dispute, DisputeResponse, DisputeStatus, DisputeView, EntityId, EvidenceRequest,
    MilestoneStatus, OpenDisputeRequest, ProjectView, SubmissionReview, UnixSeconds,
};

use super::{State, project::milestone_mut};
use crate::{Error, Result, random_id, require};

#[derive(Debug, thiserror::Error)]
pub(super) enum DisputeError {
    #[error("project_disputed")]
    ProjectFrozen,
    #[error("active_dispute_exists")]
    AlreadyOpen,
    #[error("milestone_not_changes_requested")]
    NotRejected,
    #[error("dispute_response_forbidden")]
    ResponseForbidden,
    #[error("dispute_already_answered")]
    AlreadyAnswered,
}

impl From<DisputeError> for Error {
    fn from(value: DisputeError) -> Self {
        let code = match value {
            DisputeError::ProjectFrozen => "project_disputed",
            DisputeError::AlreadyOpen => "active_dispute_exists",
            DisputeError::NotRejected => "milestone_not_changes_requested",
            DisputeError::ResponseForbidden => "dispute_response_forbidden",
            DisputeError::AlreadyAnswered => "dispute_already_answered",
        };
        Self::domain(code)
    }
}

impl State {
    pub(super) fn open_dispute(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        request: &OpenDisputeRequest,
        now: UnixSeconds,
    ) -> Result<EntityId> {
        require(
            origin == project.client || origin == project.coordinator,
            "project_party_required",
        )?;
        if project.active_dispute_id.is_some() {
            return Err(DisputeError::AlreadyOpen.into());
        }
        let revision = project
            .proposals
            .iter()
            .find(|p| {
                p.milestones
                    .iter()
                    .any(|m| m.milestone_id == request.milestone_id)
            })
            .ok_or_else(|| Error::domain("milestone_not_found"))?
            .revision;
        let milestone = milestone_mut(project, request.milestone_id)?;
        if milestone.status != Some(MilestoneStatus::ChangesRequested) {
            return Err(DisputeError::NotRejected.into());
        }
        let submission = milestone
            .submissions
            .last()
            .ok_or_else(|| Error::domain("submission_not_current"))?;
        require(
            submission.submission_id == request.rejected_submission_id,
            "submission_not_current",
        )?;
        require(
            matches!(submission.review, SubmissionReview::Rejected { .. }),
            "submission_not_rejected",
        )?;
        let id = random_id()?;
        milestone.status = Some(MilestoneStatus::Disputed);
        milestone.frozen = true;
        self.disputes.insert(
            id,
            Dispute {
                dispute_id: id,
                project_id: project.project_id,
                milestone_id: request.milestone_id,
                rejected_submission_id: request.rejected_submission_id,
                status: DisputeStatus::Open,
                opened_by: origin,
                counterparty: if origin == project.client {
                    project.coordinator
                } else {
                    project.client
                },
                opened_at: now,
                evidence: request.evidence.clone(),
                response: None,
                proposal_revision: revision,
                context_event_cursor: u64::try_from(self.events.len())
                    .map_err(|_| Error::internal())?,
            },
        );
        project.active_dispute_id = Some(id);
        Ok(id)
    }

    pub(super) fn respond_dispute(
        &mut self,
        project: &ProjectView,
        origin: AccountId32,
        id: EntityId,
        request: &EvidenceRequest,
        now: UnixSeconds,
    ) -> Result<()> {
        let dispute = self
            .disputes
            .get_mut(&id)
            .ok_or_else(|| Error::domain("dispute_not_found"))?;
        require(
            dispute.project_id == project.project_id && project.active_dispute_id == Some(id),
            "invalid_dispute_target",
        )?;
        if origin != dispute.counterparty {
            return Err(DisputeError::ResponseForbidden.into());
        }
        if dispute.response.is_some() {
            return Err(DisputeError::AlreadyAnswered.into());
        }
        dispute.response = Some(DisputeResponse {
            evidence: request.evidence.clone(),
            author: origin,
            occurred_at: now,
        });
        Ok(())
    }

    pub(crate) fn dispute_view(&self, id: EntityId) -> Result<DisputeView> {
        let dispute = self
            .disputes
            .get(&id)
            .ok_or_else(|| Error::domain("dispute_not_found"))?;
        let milestone = self
            .projects
            .get(&dispute.project_id)
            .and_then(|p| {
                p.proposals
                    .iter()
                    .flat_map(|p| &p.milestones)
                    .find(|m| m.milestone_id == dispute.milestone_id)
            })
            .ok_or_else(Error::internal)?;
        Ok(DisputeView {
            dispute: dispute.clone(),
            milestone: milestone.clone(),
        })
    }
}
