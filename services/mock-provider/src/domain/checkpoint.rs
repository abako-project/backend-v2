//! Typed decoding of the current native checkpoint.
use generated_contracts::{
    AccountId32, CompensationPolicy, Dispute, DomainEvent, EntityId, Money, OperationId,
    ProviderInfo, ScorePolicy,
    provider_core::{
        CatalogEntryCore, ProjectCore, ReasonCore, SkillMetadataCore, SkillRequestCore, WorkerCore,
    },
};
use provider_execution::RecordedCall;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    /// Retained generation, root account and payload version.
    pub info: ProviderInfo,
    /// Role catalog keyed by public ID.
    pub roles: BTreeMap<u32, CatalogEntryCore>,
    /// Skill catalog keyed by public ID.
    pub skills: BTreeMap<u32, CatalogEntryCore>,
    /// Retained category and role associations for each skill.
    pub skill_metadata: BTreeMap<u32, SkillMetadataCore>,
    /// All moderated skill requests, including their outcomes.
    pub skill_requests: BTreeMap<EntityId, SkillRequestCore>,
    /// Current vote weights and legacy blending policy.
    pub score_policy: ScorePolicy,
    /// Current proposal allocation policy.
    pub compensation_policy: CompensationPolicy,
    /// All workers, calendars, reservations and exact score accumulators.
    pub workers: BTreeMap<AccountId32, WorkerCore>,
    /// All Projects and their retained nested histories.
    pub projects: BTreeMap<EntityId, ProjectCore>,
    /// Visible free asset balances, including retained zero entries.
    pub balances: BTreeMap<AccountId32, Money>,
    /// Next signed operation nonce per origin.
    pub nonces: BTreeMap<AccountId32, u64>,
    /// Exact accepted call bytes, retained inputs and final receipts.
    pub receipts: BTreeMap<OperationId, RecordedCall>,
    /// Complete ordered domain event history.
    pub events: Vec<DomainEvent>,
    /// Private rejection and cancellation reasons in original order.
    pub reasons: Vec<ReasonCore>,
    /// All case records, responses and captured context.
    pub disputes: BTreeMap<EntityId, Dispute>,
    /// All deposit confirmations and withdrawal holds.
    pub bramp: super::bramp::BrampState,
    /// Next global task key, including gaps left by deleted tasks.
    pub next_task_id: u32,
    /// Total visible issued asset supply.
    pub minted_units: Money,
    /// Native state interpretation version.
    pub engine_version: u32,
}
use super::{RustEngine, State, execution};
use std::collections::BTreeMap;
impl From<Checkpoint> for State {
    fn from(saved: Checkpoint) -> Self {
        let root = saved.info.root_account;
        Self {
            shell: execution::Shell {
                info: saved.info,
                nonces: saved.nonces,
                receipts: saved.receipts,
                events: saved.events,
            },
            engine_version: saved.engine_version,
            engine: RustEngine {
                roles: saved.roles,
                skills: saved.skills,
                skill_metadata: saved.skill_metadata,
                skill_requests: saved.skill_requests,
                score_policy: saved.score_policy,
                compensation_policy: saved.compensation_policy,
                workers: saved.workers,
                projects: saved.projects,
                balances: saved.balances,
                reasons: saved.reasons,
                disputes: saved.disputes,
                bramp: saved.bramp,
                next_task_id: saved.next_task_id,
                minted_units: saved.minted_units,
                skill_index: BTreeMap::new(),
                context: execution::EngineContext::new(root),
            },
        }
    }
}

#[cfg(feature = "storage-sqlite")]
impl RustEngine {
    // Restarts must retain deleted-key gaps while preventing reuse of a global task key.
    pub(super) fn validate_task_keys(&self) -> crate::Result<()> {
        let mut keys = std::collections::BTreeSet::new();
        crate::require(self.next_task_id > 0, "invalid_task_state")?;
        for task in self
            .projects
            .values()
            .flat_map(|project| &project.proposals)
            .flat_map(|proposal| &proposal.milestones)
            .flat_map(|milestone| &milestone.task_storage.tasks)
        {
            crate::require(
                task.task_id > 0 && task.task_id < self.next_task_id && keys.insert(task.task_id),
                "invalid_task_state",
            )?;
        }
        Ok(())
    }
}
