//! Private task migration data, captured from one selected provider state.
use crate::{ContractError, EntityId, ProviderInstanceId, TaskStorageView};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Task data and the actual retained allocator, including deleted-ID gaps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskStateV1 {
    /// Version of this owner-provided read format, independent of signed calls.
    pub schema_version: u16,
    /// Generation that scopes all entity and task IDs.
    pub generation: ProviderInstanceId,
    /// Actual next global key; `u32::MAX` represents exhaustion, never wraparound.
    pub next_task_id: u32,
    /// Storages use deterministic ID order; each task-vector order is retained exactly.
    pub storages: Vec<TaskStorageRecordV1>,
}
/// The core context that owns a task storage, including empty storages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskStorageRecordV1 {
    /// Owning Project, independent of task assignment/visibility.
    pub project_id: EntityId,
    /// Owning proposal, without any presentation or financial rewrite.
    pub proposal_id: EntityId,
    /// Confirmed proposal revision at capture.
    pub proposal_revision: u64,
    /// Original storage/milestone IDs, tasks, authors and timestamps.
    pub storage: TaskStorageView,
}
impl TaskStateV1 {
    /// Check identity/context integrity without adding new task content rules.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 1 || self.next_task_id == 0 {
            return Err(ContractError::Invalid("task_state"));
        }
        let mut storages = BTreeSet::new();
        let mut contexts = BTreeSet::new();
        let mut revisions = BTreeMap::new();
        let mut tasks = BTreeSet::new();
        for record in &self.storages {
            let key = (record.project_id, record.proposal_id);
            if !storages.insert(record.storage.task_storage_id)
                || !contexts.insert((key, record.storage.milestone_id))
                || revisions
                    .insert(key, record.proposal_revision)
                    .is_some_and(|previous| previous != record.proposal_revision)
            {
                return Err(ContractError::Invalid("task_storage_context"));
            }
            for task in &record.storage.tasks {
                if task.task_id == 0
                    || task.task_id >= self.next_task_id
                    || !tasks.insert(task.task_id)
                {
                    return Err(ContractError::Invalid("task_key"));
                }
            }
        }
        Ok(())
    }
}

/// Private execution fact used to materialize a confirmed compact task creation.
/// The public receipt continues to identify the task storage, not the task key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskAllocationV1 {
    /// The original successful provider receipt, unchanged.
    pub receipt: crate::OperationReceipt,
    /// Owning project from the retained signed command.
    pub project_id: EntityId,
    /// Owning storage from that same command and receipt.
    pub task_storage_id: EntityId,
    /// Key returned by the core execution that produced the receipt.
    pub task_id: u32,
}
