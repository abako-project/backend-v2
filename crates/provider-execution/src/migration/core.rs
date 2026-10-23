//! Current archive format. V1 remains only the explicit predecessor exchange.
use super::{BrampArchive, ReasonRecord, SkillMetadata};
use crate::{RecordedCall, Shell};
use generated_contracts::{
    AccountId32, CatalogEntry, CompensationPolicy, Dispute, DomainEvent, EntityId, Money,
    OperationId, ProviderInfo, ScorePolicy, SkillRequestView,
    provider_core::{ProjectCore, WorkerCore},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One complete private provider generation, including accepted operation history.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreArchiveV2 {
    /// Archive format version; version 2 for compact creation state.
    pub version: u32,
    /// Complete typed native-compatible state.
    pub checkpoint: CoreCheckpointV2,
}
/// Current compact native layout; conversion from the immediate predecessor is explicit.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreCheckpointV2 {
    /// Retained generation, root account and payload version.
    pub info: ProviderInfo,
    /// Role catalog keyed by public ID.
    pub roles: BTreeMap<u32, CatalogEntry>,
    /// Skill catalog keyed by public ID.
    pub skills: BTreeMap<u32, CatalogEntry>,
    /// Retained category and role associations for each skill.
    pub skill_metadata: BTreeMap<u32, SkillMetadata>,
    /// All moderated skill requests, including their outcomes.
    pub skill_requests: BTreeMap<EntityId, SkillRequestView>,
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
    pub reasons: Vec<ReasonRecord>,
    /// All case records, responses and captured context.
    pub disputes: BTreeMap<EntityId, Dispute>,
    /// All deposit confirmations and withdrawal holds.
    pub bramp: BrampArchive,
    /// Next global task key, including gaps left by deleted tasks.
    pub next_task_id: u32,
    /// Total visible issued asset supply.
    pub minted_units: Money,
    /// Native state interpretation version.
    pub engine_version: u32,
}
impl CoreCheckpointV2 {
    /// Copy the shell without deriving history from public events or receipts.
    #[must_use]
    pub fn shell(&self) -> Shell {
        Shell {
            info: self.info.clone(),
            nonces: self.nonces.clone(),
            receipts: self.receipts.clone(),
            events: self.events.clone(),
        }
    }
}
