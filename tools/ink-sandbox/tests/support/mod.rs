//! Explicit task-content handoff for older metadata tests; no legacy executor.
use generated_contracts::TaskStorageView;
use generated_contracts::provider_core::{TaskStateV1, TaskStorageRecordV1};
use provider_execution::migration::{CoreArchiveV4, CoreArchiveV5};

pub struct TaskContent(TaskStateV1);
impl TaskContent {
    pub fn split(previous: CoreArchiveV4) -> (CoreArchiveV5, Self) {
        let (core, tasks) = mock_provider::migration::compact_tasks(
            previous.checkpoint.info.root_account,
            previous,
        )
        .unwrap();
        (core, Self(tasks))
    }
    /// Metadata scenarios never edit tasks. Preserve existing content and admit
    /// only empty new storages when proposals add or replace milestone contexts.
    pub fn restore(&self, core: CoreArchiveV5) -> CoreArchiveV4 {
        let mut tasks = TaskStateV1 {
            schema_version: 1,
            generation: core.checkpoint.info.provider_instance_id,
            next_task_id: core.checkpoint.next_task_id,
            storages: Vec::new(),
        };
        for project in core.checkpoint.projects.values() {
            for proposal in &project.proposals {
                for milestone in &proposal.milestones {
                    let retained = self.0.storages.iter().find(|r| {
                        r.storage.task_storage_id == milestone.task_storage.task_storage_id
                    });
                    let storage = retained.map_or_else(
                        || {
                            assert!(milestone.task_storage.tasks.is_empty());
                            TaskStorageView {
                                task_storage_id: milestone.task_storage.task_storage_id,
                                milestone_id: milestone.milestone_id,
                                tasks: vec![],
                            }
                        },
                        |r| r.storage.clone(),
                    );
                    tasks.storages.push(TaskStorageRecordV1 {
                        project_id: project.project_id,
                        proposal_id: proposal.proposal_id,
                        proposal_revision: proposal.revision,
                        storage,
                    });
                }
            }
        }
        tasks.storages.sort_by_key(|r| r.storage.task_storage_id);
        core.into_v4(tasks).unwrap()
    }
}
