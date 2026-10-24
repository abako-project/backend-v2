//! The flat version-1 storage representation predates the engine boundary.
//! Keep typed JSON decoding here: serde flatten buffers map keys as strings and
//! cannot decode the existing integer-keyed catalogs through that buffer.
use super::{RustEngine, State, execution};
pub(super) use provider_execution::migration::CoreCheckpointV4;
use std::collections::BTreeMap;
impl From<CoreCheckpointV4> for State {
    fn from(saved: CoreCheckpointV4) -> Self {
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
                bramp: saved.bramp.into(),
                next_task_id: saved.next_task_id,
                minted_units: saved.minted_units,
                skill_index: BTreeMap::new(),
                context: execution::EngineContext::new(root),
            },
        }
    }
}

impl RustEngine {
    // Import must retain deleted-key gaps while preventing reuse of a global task key.
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
