//! Private, versioned migration archive. Never expose it through the HTTP API.
//! Typed fields preserve u128 accumulators and data omitted from public snapshots.
mod core;
pub use core::{CoreArchiveV2, CoreCheckpointV2};
mod core_v3;
pub use core_v3::{CoreArchiveV3, CoreCheckpointV3};

use crate::{RecordedCall, Shell};
use generated_contracts::{
    AccountId32, CatalogEntry, CompensationPolicy, DepositView, Dispute, DomainEvent,
    DomainEventKind, EntityId, Money, OperationId, ProjectView, ProviderInfo, ScorePolicy,
    SkillRequestView, UnixSeconds, WithdrawalView, WorkerView,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One complete private provider generation, including accepted operation history.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveV1 {
    /// Archive format version; only version 1 is supported.
    pub version: u32,
    /// Complete typed native-compatible state.
    pub checkpoint: CheckpointV1,
}
/// Existing flat native storage layout. Defaults only cover documented older v1 fields.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointV1 {
    /// Retained generation, root account and payload version.
    pub info: ProviderInfo,
    /// Role catalog keyed by public ID.
    pub roles: BTreeMap<u32, CatalogEntry>,
    /// Skill catalog keyed by public ID.
    pub skills: BTreeMap<u32, CatalogEntry>,
    /// Retained category and role associations for each skill.
    pub skill_metadata: BTreeMap<u32, SkillMetadata>,
    #[serde(default)]
    /// All moderated skill requests, including their outcomes.
    pub skill_requests: BTreeMap<EntityId, SkillRequestView>,
    /// Current vote weights and legacy blending policy.
    pub score_policy: ScorePolicy,
    #[serde(default)]
    /// Current proposal allocation policy.
    pub compensation_policy: CompensationPolicy,
    /// All workers, calendars, reservations and exact score accumulators.
    pub workers: BTreeMap<AccountId32, WorkerView>,
    /// All Projects and their retained nested histories.
    pub projects: BTreeMap<EntityId, ProjectView>,
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
    #[serde(default)]
    /// All deposit confirmations and withdrawal holds.
    pub bramp: BrampArchive,
    /// Next global task key, including gaps left by deleted tasks.
    pub next_task_id: u32,
    /// Total visible issued asset supply.
    pub minted_units: Money,
    #[serde(default = "engine_version")]
    /// Native state interpretation version.
    pub engine_version: u32,
}
const fn engine_version() -> u32 {
    1
}
impl CheckpointV1 {
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Retained native skill metadata, separate from the public catalog view.
pub struct SkillMetadata {
    /// Retained descriptive category; it does not restrict matching.
    pub category: String,
    /// Associated role IDs in retained order.
    pub role_ids: Vec<u32>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Private historical explanation of a rejection or cancellation.
pub struct ReasonRecord {
    /// Affected public Project ID.
    pub project_id: EntityId,
    /// Affected milestone, when present.
    pub milestone_id: Option<EntityId>,
    /// Verified actor that recorded the reason.
    pub origin: AccountId32,
    /// Original private reason text.
    pub reason: String,
    /// Captured timestamp of the original effect.
    pub occurred_at: UnixSeconds,
    /// Original effect kind.
    pub kind: DomainEventKind,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Complete Bramp write model; pending withdrawals retain their held funds.
pub struct BrampArchive {
    /// Deposit records keyed by public deposit ID.
    pub deposits: BTreeMap<EntityId, DepositView>,
    /// Withdrawal records keyed by public withdrawal ID.
    pub withdrawals: BTreeMap<EntityId, WithdrawalView>,
}
