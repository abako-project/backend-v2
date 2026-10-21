//! The flat version-1 storage representation predates the engine boundary.
//! Keep typed JSON decoding here: serde flatten buffers map keys as strings and
//! cannot decode the existing integer-keyed catalogs through that buffer.
use super::{RecordedCall, RecordedReason, RustEngine, State, bramp, execution};
use crate::seed::SkillMetadata;
use generated_contracts::{
    AccountId32, CatalogEntry, DomainEvent, EntityId, Money, OperationId, ProjectView,
    ProviderInfo, ScorePolicy, SkillRequestView, WorkerView,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub(super) struct CheckpointV1 {
    info: ProviderInfo,
    roles: BTreeMap<u32, CatalogEntry>,
    skills: BTreeMap<u32, CatalogEntry>,
    skill_metadata: BTreeMap<u32, SkillMetadata>,
    #[serde(default)]
    skill_requests: BTreeMap<EntityId, SkillRequestView>,
    score_policy: ScorePolicy,
    #[serde(default)]
    compensation_policy: generated_contracts::CompensationPolicy,
    workers: BTreeMap<AccountId32, WorkerView>,
    projects: BTreeMap<EntityId, ProjectView>,
    balances: BTreeMap<AccountId32, Money>,
    nonces: BTreeMap<AccountId32, u64>,
    receipts: BTreeMap<OperationId, RecordedCall>,
    events: Vec<DomainEvent>,
    reasons: Vec<RecordedReason>,
    disputes: BTreeMap<EntityId, generated_contracts::Dispute>,
    #[serde(default)]
    bramp: bramp::BrampState,
    next_task_id: u32,
    minted_units: Money,

    #[serde(default = "super::rust_engine_version")]
    engine_version: u32,
}
impl From<CheckpointV1> for State {
    fn from(saved: CheckpointV1) -> Self {
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
