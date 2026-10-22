//! The flat version-1 storage representation predates the engine boundary.
//! Keep typed JSON decoding here: serde flatten buffers map keys as strings and
//! cannot decode the existing integer-keyed catalogs through that buffer.
use super::{RustEngine, State, execution};
pub(super) use provider_execution::migration::CheckpointV1;
use std::collections::BTreeMap;
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
                skill_metadata: saved
                    .skill_metadata
                    .into_iter()
                    .map(|(id, metadata)| (id, metadata.into()))
                    .collect(),
                skill_requests: saved.skill_requests,
                score_policy: saved.score_policy,
                compensation_policy: saved.compensation_policy,
                workers: saved.workers,
                projects: saved.projects,
                balances: saved.balances,
                reasons: saved.reasons,
                disputes: saved.disputes,
                bramp: saved.bramp.into(),
                next_task_id: saved.next_task_id,
                minted_units: saved.minted_units,
                skill_index: BTreeMap::new(),
                context: execution::EngineContext::new(root),
            },
        }
    }
}
