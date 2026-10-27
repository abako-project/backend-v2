//! R2a core read/storage model. The adapter supplies creation content only after
//! selecting an authorized entity and verifying its immutable record.
use super::MetadataCommitmentV1;
use crate::{
    AccountId32, BalanceView, CalendarView, EntityId, Money, PayloadHash, PlanningView,
    ProjectEvaluation, ProjectView, ProposalView, ProviderInfo, Qualifications, ReputationView,
    UnixSeconds, WorkerMode, WorkerView,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Signed creation references select API-owned descriptive content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum MetadataReferenceV1 {
    /// Immutable content bound to the original creation envelope.
    #[codec(index = 0)]
    Creation(MetadataCommitmentV1),
}
impl MetadataReferenceV1 {
    /// Expected digest of the exact offchain record.
    #[must_use]
    pub const fn hash(&self) -> PayloadHash {
        match self {
            Self::Creation(content) => content.hash,
        }
    }
}

macro_rules! core_dto {
    ($(#[$attr:meta])* $name:ident { $($field:ident:$ty:ty),* $(,)? }) => {
        $(#[$attr])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(
            #[doc = concat!("Core field `", stringify!($field), "`.")]
            pub $field: $ty,
        )* }
    };
}
core_dto!(/// Worker business facts with an immutable directory-name commitment.
WorkerCore {
    account: AccountId32, content: MetadataReferenceV1,
    qualifications: Qualifications, mode: WorkerMode, coordinator_eligible: bool,
    calendar: CalendarView, worker_score: ReputationView, coordinator_score: ReputationView
});
core_dto!(/// Project business facts. Proposal/task children move in subsequent slices.
ProjectCore {
    project_id: EntityId, created_at: Option<UnixSeconds>, client: AccountId32,
    coordinator: AccountId32, content: MetadataReferenceV1, planning: PlanningView,
    proposals: Vec<super::ProposalCore>, execution_escrow: Money, cancelled: bool,
    completed: bool, evaluations: Vec<ProjectEvaluation>, active_dispute_id: Option<EntityId>
});
impl WorkerCore {
    /// Assemble only with the exact verified directory-name preimage.
    #[must_use]
    pub fn into_view(self, display_name: String) -> WorkerView {
        WorkerView {
            account: self.account,
            display_name,
            qualifications: self.qualifications,
            mode: self.mode,
            coordinator_eligible: self.coordinator_eligible,
            calendar: self.calendar,
            worker_score: self.worker_score,
            coordinator_score: self.coordinator_score,
        }
    }
}
impl ProjectCore {
    /// Assemble only with verified root content; current proposal/task children
    /// come from this core record, never from a stale content-copy snapshot.
    #[must_use]
    pub fn into_view(
        self,
        title: String,
        description: String,
        proposals: Vec<ProposalView>,
    ) -> ProjectView {
        ProjectView {
            project_id: self.project_id,
            created_at: self.created_at,
            client: self.client,
            coordinator: self.coordinator,
            title,
            description,
            planning: self.planning,
            proposals,
            execution_escrow: self.execution_escrow,
            cancelled: self.cancelled,
            completed: self.completed,
            evaluations: self.evaluations,
            active_dispute_id: self.active_dispute_id,
        }
    }
}

core_dto!(/// Current core facts, with task identity and membership only.
CoreSnapshot {
    schema_version: u16, info: ProviderInfo, catalog: super::CatalogCore,
    workers: Vec<WorkerCore>, projects: Vec<ProjectCore>, balances: Vec<BalanceView>
});
