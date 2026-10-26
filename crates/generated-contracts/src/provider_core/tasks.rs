//! Current core task facts and immutable API dispute snapshots.
use crate::{ContractError, EntityId, ProviderInstanceId, TaskStorageView};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Independently enforceable task identity and notification membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCore {
    /// Actual global core allocation, retained after content moves to the API.
    pub task_id: u32,
    /// Ordered, unique worker accounts used by execution-time authorization/events.
    pub assignees: Vec<crate::AccountId32>,
}

/// Ordered task identities in one core milestone context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskStorageCore {
    /// Original task-storage identity.
    pub task_storage_id: EntityId,
    /// Original milestone identity.
    pub milestone_id: EntityId,
    /// Current identities and membership; no task narrative or progress.
    pub tasks: Vec<TaskCore>,
}
impl From<&crate::TaskView> for TaskCore {
    fn from(value: &crate::TaskView) -> Self {
        Self {
            task_id: value.task_id,
            assignees: value.task.assignees.clone(),
        }
    }
}
impl From<&TaskStorageView> for TaskStorageCore {
    fn from(value: &TaskStorageView) -> Self {
        Self {
            task_storage_id: value.task_storage_id,
            milestone_id: value.milestone_id,
            tasks: value
                .tasks
                .iter()
                .map(|task| TaskCore {
                    task_id: task.task_id,
                    assignees: task.task.assignees.clone(),
                })
                .collect(),
        }
    }
}

/// Atomic private read of current core task facts, including the actual allocator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskState {
    /// Version 2 denotes identity/membership rather than full task content.
    pub schema_version: u16,
    /// Generation scopes all task and context identities.
    pub generation: ProviderInstanceId,
    /// Actual next core key, including deleted gaps and exhaustion.
    pub next_task_id: u32,
    /// Any retained task request/event, including failed or deleted work.
    /// Empty-development initialization must not reinterpret such a history.
    pub has_task_history: bool,
    /// Complete current storage contexts in deterministic ID order.
    pub storages: Vec<TaskStorageRecord>,
}

/// Exact proposal revision and milestone owning a compact task storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskStorageRecord {
    /// Owning Project.
    pub project_id: EntityId,
    /// Owning proposal.
    pub proposal_id: EntityId,
    /// Confirmed proposal revision at capture.
    pub proposal_revision: u64,
    /// Task identities and assignees from the same core read.
    pub storage: TaskStorageCore,
}

impl TaskState {
    /// Validate global allocation, distinct contexts and unique memberships.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != 2 || self.next_task_id == 0 {
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
                    || task.assignees.iter().collect::<BTreeSet<_>>().len() != task.assignees.len()
                {
                    return Err(ContractError::Invalid("task_key"));
                }
            }
        }
        Ok(())
    }
}

/// Task data and the actual retained allocator, including deleted-ID gaps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSnapshot {
    /// Version of this owner-provided read format, independent of signed calls.
    pub schema_version: u16,
    /// Generation that scopes all entity and task IDs.
    pub generation: ProviderInstanceId,
    /// Actual next global key; `u32::MAX` represents exhaustion, never wraparound.
    pub next_task_id: u32,
    /// Storages use deterministic ID order; each task-vector order is retained exactly.
    pub storages: Vec<TaskSnapshotRecord>,
}
/// The core context that owns a task storage, including empty storages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSnapshotRecord {
    /// Owning Project, independent of task assignment/visibility.
    pub project_id: EntityId,
    /// Owning proposal, without any presentation or financial rewrite.
    pub proposal_id: EntityId,
    /// Confirmed proposal revision at capture.
    pub proposal_revision: u64,
    /// Original storage/milestone IDs, tasks, authors and timestamps.
    pub storage: TaskStorageView,
}
impl TaskSnapshot {
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
pub struct TaskAllocation {
    /// The original successful provider receipt, unchanged.
    pub receipt: crate::OperationReceipt,
    /// Owning project from the retained signed command.
    pub project_id: EntityId,
    /// Owning storage from that same command and receipt.
    pub task_storage_id: EntityId,
    /// Key returned by the core execution that produced the receipt.
    pub task_id: u32,
}
