use protocol::provider_core::CoreDisputeView;
use protocol::provider_core::ProjectCore;
use protocol::{
    AccountId32, Dispute, DisputeResponse, DisputeStatus, EntityId, EvidenceRecordReferenceV1,
    MilestoneStatus, OpenDisputeRequest, OpenDisputeWithCommentRequest, SubmissionReview,
    UnixSeconds,
};

use super::{RustEngine, project::milestone_mut};
use crate::{Error, Result, require};

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

impl RustEngine {
    pub(super) fn open_dispute(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        request: &OpenDisputeRequest,
        now: UnixSeconds,
    ) -> Result<EntityId> {
        self.open_case(
            project,
            origin,
            request.milestone_id,
            request.rejected_submission_id,
            Some(request.evidence.clone().into()),
            None,
            now,
        )
    }

    pub(super) fn open_dispute_with_comment(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        request: &OpenDisputeWithCommentRequest,
        now: UnixSeconds,
    ) -> Result<EntityId> {
        self.open_case(
            project,
            origin,
            request.milestone_id,
            request.rejected_submission_id,
            None,
            Some(request.comment_id),
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_case(
        &mut self,
        project: &mut ProjectCore,
        origin: AccountId32,
        milestone_id: EntityId,
        rejected_submission_id: EntityId,
        evidence: Option<EvidenceRecordReferenceV1>,
        opening_comment_id: Option<EntityId>,
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
            .find(|p| p.milestones.iter().any(|m| m.milestone_id == milestone_id))
            .ok_or_else(|| Error::domain("milestone_not_found"))?
            .revision;
        let milestone = milestone_mut(project, milestone_id)?;
        if milestone.status != Some(MilestoneStatus::ChangesRequested) {
            return Err(DisputeError::NotRejected.into());
        }
        let submission = milestone
            .submissions
            .last()
            .ok_or_else(|| Error::domain("submission_not_current"))?;
        require(
            submission.submission_id == rejected_submission_id,
            "submission_not_current",
        )?;
        require(
            matches!(
                submission.review,
                SubmissionReview::Rejected { .. } | SubmissionReview::RejectedWithComment { .. }
            ),
            "submission_not_rejected",
        )?;
        let id = self.context.random_id();
        milestone.status = Some(MilestoneStatus::Disputed);
        milestone.frozen = true;
        self.disputes.insert(
            id,
            Dispute {
                dispute_id: id,
                project_id: project.project_id,
                milestone_id,
                rejected_submission_id,
                status: DisputeStatus::Open,
                opened_by: origin,
                counterparty: if origin == project.client {
                    project.coordinator
                } else {
                    project.client
                },
                opened_at: now,
                evidence,
                opening_comment_id,
                response: None,
                proposal_revision: revision,
                context_event_cursor: self.context.event_cursor,
            },
        );
        project.active_dispute_id = Some(id);
        Ok(id)
    }

    pub(super) fn respond_dispute(
        &mut self,
        project: &ProjectCore,
        origin: AccountId32,
        id: EntityId,
        evidence: EvidenceRecordReferenceV1,
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
            evidence,
            author: origin,
            occurred_at: now,
        });
        Ok(())
    }

    pub(crate) fn dispute_view(&self, id: EntityId) -> Result<CoreDisputeView> {
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
            .ok_or_else(Error::unavailable)?;
        Ok(CoreDisputeView {
            dispute: dispute.clone(),
            milestone: milestone.clone(),
        })
    }
}
