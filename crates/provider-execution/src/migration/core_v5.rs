//! Current compact-task core archive. V4 is the immediate predecessor exchange.
use super::BrampArchive;
use crate::{RecordedCall, Shell};
use generated_contracts::{
    AccountId32, CompensationPolicy, Dispute, DomainEvent, EntityId, Money, OperationId,
    ProviderInfo, ScorePolicy,
    provider_core::{
        CatalogEntryCore, ProjectCore, ReasonCore, SkillMetadataCore, SkillRequestCore, WorkerCore,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One complete private provider generation, including accepted operation history.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreArchiveV5 {
    /// Archive format version; version 5 for compact task state.
    pub version: u32,
    /// Complete typed native-compatible state.
    pub checkpoint: CoreCheckpointV5,
}
/// Current compact native layout; conversion from the immediate predecessor is explicit.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreCheckpointV5 {
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
    pub bramp: BrampArchive,
    /// Next global task key, including gaps left by deleted tasks.
    pub next_task_id: u32,
    /// Total visible issued asset supply.
    pub minted_units: Money,
    /// Native state interpretation version.
    pub engine_version: u32,
}
impl CoreCheckpointV5 {
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

impl CoreArchiveV5 {
    /// Split one predecessor archive into compact facts and a lossless API task export.
    /// No signer, timestamp or commitment is invented by this conversion.
    pub fn from_v4(
        archive: super::CoreArchiveV4,
    ) -> crate::Result<(Self, generated_contracts::provider_core::TaskStateV1)> {
        use generated_contracts::provider_core::{TaskStateV1, TaskStorageRecordV1};
        crate::require(
            archive.version == 4 && archive.checkpoint.engine_version == 4,
            "invalid_migration_archive",
        )?;
        let old = archive.checkpoint;
        old.shell().validate(old.info.root_account)?;
        let mut tasks = TaskStateV1 {
            schema_version: 1,
            generation: old.info.provider_instance_id,
            next_task_id: old.next_task_id,
            storages: Vec::new(),
        };
        for project in old.projects.values() {
            for proposal in &project.proposals {
                for milestone in &proposal.milestones {
                    tasks.storages.push(TaskStorageRecordV1 {
                        project_id: project.project_id,
                        proposal_id: proposal.proposal_id,
                        proposal_revision: proposal.revision,
                        storage: milestone.task_storage.clone(),
                    });
                }
            }
        }
        tasks
            .storages
            .sort_by_key(|record| record.storage.task_storage_id);
        tasks.validate()?;
        for task in tasks
            .storages
            .iter()
            .flat_map(|record| &record.storage.tasks)
        {
            task.task.validate()?;
        }
        let projects = old
            .projects
            .into_iter()
            .map(|(id, value)| (id, value.into()))
            .collect();
        let core = Self {
            version: 5,
            checkpoint: CoreCheckpointV5 {
                info: old.info,
                roles: old.roles,
                skills: old.skills,
                skill_metadata: old.skill_metadata,
                skill_requests: old.skill_requests,
                score_policy: old.score_policy,
                compensation_policy: old.compensation_policy,
                workers: old.workers,
                balances: old.balances,
                nonces: old.nonces,
                receipts: old.receipts,
                events: old.events,
                reasons: old.reasons,
                disputes: old.disputes,
                bramp: old.bramp,
                next_task_id: old.next_task_id,
                minted_units: old.minted_units,
                projects,
                engine_version: 5,
            },
        };
        Ok((core, tasks))
    }

    /// Restore exact current API task content to the immediate predecessor.
    /// Actual core identities, assignees and counter must match; content cannot
    /// change authority facts during rollback.
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the explicit immediate-predecessor field transfer and content matching auditable together."
    )]
    pub fn into_v4(
        self,
        tasks: generated_contracts::provider_core::TaskStateV1,
    ) -> crate::Result<super::CoreArchiveV4> {
        use generated_contracts::provider_core::{
            MilestoneCoreV4, ProjectCoreV4, ProposalCoreV4, TaskStorageCore,
        };
        crate::require(
            self.version == 5 && self.checkpoint.engine_version == 5,
            "invalid_migration_archive",
        )?;
        tasks.validate()?;
        let old = self.checkpoint;
        old.shell().validate(old.info.root_account)?;
        crate::require(
            tasks.generation == old.info.provider_instance_id
                && tasks.next_task_id == old.next_task_id,
            "task_restore_generation_or_counter",
        )?;
        let mut records: BTreeMap<_, _> = tasks
            .storages
            .into_iter()
            .map(|record| (record.storage.task_storage_id, record))
            .collect();
        let mut projects = BTreeMap::new();
        for (id, project) in old.projects {
            let mut proposals = Vec::new();
            for proposal in project.proposals {
                let mut milestones = Vec::new();
                for milestone in proposal.milestones {
                    let record = records
                        .remove(&milestone.task_storage.task_storage_id)
                        .ok_or_else(|| crate::Error::invalid("task_restore_context"))?;
                    crate::require(
                        record.project_id == project.project_id
                            && record.proposal_id == proposal.proposal_id
                            && record.proposal_revision == proposal.revision
                            && record.storage.milestone_id == milestone.milestone_id
                            && TaskStorageCore::from(&record.storage) == milestone.task_storage,
                        "task_restore_context",
                    )?;
                    for task in &record.storage.tasks {
                        task.task.validate()?;
                    }
                    milestones.push(MilestoneCoreV4 {
                        milestone_id: milestone.milestone_id,
                        definition: milestone.definition,
                        status: milestone.status,
                        assignments: milestone.assignments,
                        worker_ratings: milestone.worker_ratings,
                        task_storage: record.storage,
                        frozen: milestone.frozen,
                        submissions: milestone.submissions,
                    });
                }
                proposals.push(ProposalCoreV4 {
                    proposal_id: proposal.proposal_id,
                    revision: proposal.revision,
                    content: proposal.content,
                    status: proposal.status,
                    milestones,
                    change_request: proposal.change_request,
                    delivery: proposal.delivery,
                    compensation_policy: proposal.compensation_policy,
                });
            }
            projects.insert(
                id,
                ProjectCoreV4 {
                    project_id: project.project_id,
                    created_at: project.created_at,
                    client: project.client,
                    coordinator: project.coordinator,
                    content: project.content,
                    planning: project.planning,
                    proposals,
                    execution_escrow: project.execution_escrow,
                    cancelled: project.cancelled,
                    completed: project.completed,
                    evaluations: project.evaluations,
                    active_dispute_id: project.active_dispute_id,
                },
            );
        }
        crate::require(records.is_empty(), "task_restore_context")?;
        Ok(super::CoreArchiveV4 {
            version: 4,
            checkpoint: super::CoreCheckpointV4 {
                info: old.info,
                roles: old.roles,
                skills: old.skills,
                skill_metadata: old.skill_metadata,
                skill_requests: old.skill_requests,
                score_policy: old.score_policy,
                compensation_policy: old.compensation_policy,
                workers: old.workers,
                balances: old.balances,
                nonces: old.nonces,
                receipts: old.receipts,
                events: old.events,
                reasons: old.reasons,
                disputes: old.disputes,
                bramp: old.bramp,
                next_task_id: old.next_task_id,
                minted_units: old.minted_units,
                projects,
                engine_version: 4,
            },
        })
    }
}
