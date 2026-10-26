//! Full-task predecessor DTOs. No live engine uses this module.
use super::{MilestoneTerms, ProposalContentReferenceV1};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Predecessor milestone record used only for explicit data exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MilestoneCoreV4 {
    /// Provider-generated identity retained by definition key on edit.
    pub milestone_id: crate::EntityId,
    /// Enforceable terms without the display title.
    pub definition: MilestoneTerms,
    /// Contractual lifecycle state.
    pub status: Option<crate::MilestoneStatus>,
    /// Contractual assignments; distinct from task assignees.
    pub assignments: Vec<crate::AssignmentView>,
    /// Exact scores retained for settlement.
    pub worker_ratings: Vec<crate::WorkerRating>,
    /// Existing tasks remain unchanged until R3.
    pub task_storage: crate::TaskStorageView,
    /// Core freeze flag.
    pub frozen: bool,
    /// Complete submission history/evidence commitments.
    pub submissions: Vec<crate::CompletionSubmission>,
}
/// Proposal terms, selected content references and current revision/lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalCoreV4 {
    /// Provider-assigned identity.
    pub proposal_id: crate::EntityId,
    /// Actual authoritative revision; not predicted by metadata staging.
    pub revision: u64,
    /// Current definition preimage, independent from change request.
    pub content: ProposalContentReferenceV1,
    /// Current contractual state.
    pub status: crate::ProposalStatus,
    /// Ordered enforceable milestone facts.
    pub milestones: Vec<MilestoneCoreV4>,
    /// Latest request reference, preserved through ordinary draft updates.
    pub change_request: Option<ProposalContentReferenceV1>,
    /// Includes the exact date constraint used by submission validation.
    pub delivery: Option<crate::DeliveryPreference>,
    /// Policy captured atomically when quoted terms were allocated.
    pub compensation_policy: Option<crate::CompensationPolicy>,
}

impl From<MilestoneCoreV4> for super::MilestoneCore {
    fn from(value: MilestoneCoreV4) -> Self {
        Self {
            milestone_id: value.milestone_id,
            definition: value.definition,
            status: value.status,
            assignments: value.assignments,
            worker_ratings: value.worker_ratings,
            task_storage: super::TaskStorageCore::from(&value.task_storage),
            frozen: value.frozen,
            submissions: value.submissions,
        }
    }
}
impl From<ProposalCoreV4> for super::ProposalCore {
    fn from(value: ProposalCoreV4) -> Self {
        Self {
            proposal_id: value.proposal_id,
            revision: value.revision,
            content: value.content,
            status: value.status,
            milestones: value.milestones.into_iter().map(Into::into).collect(),
            change_request: value.change_request,
            delivery: value.delivery,
            compensation_policy: value.compensation_policy,
        }
    }
}
impl From<super::ProjectCoreV4> for super::ProjectCore {
    fn from(value: super::ProjectCoreV4) -> Self {
        Self {
            project_id: value.project_id,
            created_at: value.created_at,
            client: value.client,
            coordinator: value.coordinator,
            content: value.content,
            planning: value.planning,
            proposals: value.proposals.into_iter().map(Into::into).collect(),
            execution_escrow: value.execution_escrow,
            cancelled: value.cancelled,
            completed: value.completed,
            evaluations: value.evaluations,
            active_dispute_id: value.active_dispute_id,
        }
    }
}

/// Predecessor case body used only for explicit task migration and rollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoreDisputeViewV4 {
    /// Retained adjudication facts.
    pub dispute: crate::Dispute,
    /// Full predecessor task content frozen with the selected milestone.
    pub milestone: MilestoneCoreV4,
}
