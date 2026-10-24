//! Read retained API task history without turning it into provider work.
//! This is not a task writer or a task authority switch.
use crate::state::{Error, parse};
use generated_contracts::{
    AccountId32, AdapterTaskEvent, EntityId, ExecutionOutcome, OperationId, ProviderCommand,
    ProviderInstanceId, ProviderOperationStatus, TaskEventKind, TaskOperationResult,
};
use sqlx::{Row, postgres::PgRow};

pub(crate) struct TaskOperation {
    id: OperationId,
    generation: ProviderInstanceId,
    account: AccountId32,
    project: EntityId,
    storage: EntityId,
    kind: TaskEventKind,
    pub(crate) result: Option<TaskOperationResult>,
}
impl TaskOperation {
    /// Use retained operation facts, never current provider state, to bind history.
    pub(crate) fn read(row: &PgRow) -> Result<Self, Error> {
        if row.try_get::<Option<bool>, _>("task_operation")? != Some(true) {
            return Err(Error::Dependency);
        }
        let command: ProviderCommand =
            serde_json::from_str(row.try_get("command_json")?).map_err(|_| Error::Dependency)?;
        let (project, storage, kind) = match command {
            ProviderCommand::CreateTask {
                project_id,
                task_storage_id,
                ..
            } => (project_id, task_storage_id, TaskEventKind::Created),
            ProviderCommand::DeleteTask {
                project_id,
                task_storage_id,
                ..
            } => (project_id, task_storage_id, TaskEventKind::Deleted),
            ProviderCommand::EditTask {
                project_id,
                task_storage_id,
                ..
            }
            | ProviderCommand::UpdateTaskProgress {
                project_id,
                task_storage_id,
                ..
            } => (project_id, task_storage_id, TaskEventKind::Updated),
            _ => return Err(Error::Dependency),
        };
        let status: ProviderOperationStatus = serde_json::from_value(serde_json::Value::String(
            row.try_get::<String, _>("status")?,
        ))
        .map_err(|_| Error::Dependency)?;
        let result: Option<TaskOperationResult> = row
            .try_get::<Option<&str>, _>("task_result_json")?
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| Error::Dependency)?;
        if !matches!(
            status,
            ProviderOperationStatus::Queued
                | ProviderOperationStatus::Finalized
                | ProviderOperationStatus::Rejected
                | ProviderOperationStatus::Expired
        ) || (status == ProviderOperationStatus::Finalized) != result.is_some()
            || result
                .as_ref()
                .is_some_and(|r| r.task_storage_id != storage)
        {
            return Err(Error::Dependency);
        }
        Ok(Self {
            id: parse(row.try_get("operation_id")?).map_err(|_| Error::Dependency)?,
            generation: parse(row.try_get("operation_instance")?).map_err(|_| Error::Dependency)?,
            account: parse(row.try_get("operation_account")?).map_err(|_| Error::Dependency)?,
            project,
            storage,
            kind,
            result,
        })
    }

    pub(crate) fn validate_event(&self, event: &AdapterTaskEvent) -> Result<(), Error> {
        let Some(result) = &self.result else {
            return Err(Error::Dependency);
        };
        if result.outcome != ExecutionOutcome::Success
            || event.operation_id != self.id
            || event.provider_instance_id != self.generation
            || event.origin != self.account
            || event.project_id != self.project
            || event.task_storage_id != self.storage
            || event.kind != self.kind
            || event.occurred_at != result.finalized_at
        {
            return Err(Error::Dependency);
        }
        Ok(())
    }
}
