use generated_contracts::provider_core::{
    CatalogCore, CatalogEntryCore, CoreSnapshotV5, ProjectCore, SkillRequestCore, SkillRoleCore,
    WorkerCore,
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use generated_contracts::{
    AccountId32, AccountNonce, BalanceView, DomainEventKind, EntityId, Minutes, Money, OperationId,
    OperationReceipt, Percentage, ProviderEvents, ProviderInfo, ReputationView, ScorePolicy,
};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, require, seed::SkillMetadata};

mod bramp;
mod catalog_history;
mod catalog_requests;
mod checkpoint;
mod commands;
mod dispute_validation;
mod disputes;
mod evaluations;
mod execution;
pub(crate) use execution::VerifiedCall;
mod project;
mod submissions;
#[cfg(test)]
mod tests;
mod validation;

use provider_execution::Effect;

use generated_contracts::provider_core::ReasonCore as RecordedReason;
use provider_execution::migration::CoreArchiveV5;

/// Private aggregate: only authenticated calls can obtain a transition and only
/// storage can commit it. HTTP snapshots are copies, never mutable handles.
#[derive(Clone, Serialize, Deserialize)]
#[serde(from = "checkpoint::CoreCheckpointV5")]
pub(crate) struct State {
    #[serde(flatten)]
    shell: execution::Shell,
    #[serde(default = "rust_engine_version")]
    engine_version: u32,
    #[serde(flatten)]
    engine: RustEngine,
}

/// Business state has no signatures, nonces, receipts or durable event log.
#[derive(Clone, Serialize)]
struct RustEngine {
    roles: BTreeMap<u32, CatalogEntryCore>,
    skills: BTreeMap<u32, CatalogEntryCore>,
    skill_metadata: BTreeMap<u32, SkillMetadata>,
    #[serde(default)]
    skill_requests: BTreeMap<EntityId, SkillRequestCore>,
    score_policy: ScorePolicy,
    #[serde(default)]
    compensation_policy: generated_contracts::CompensationPolicy,
    workers: BTreeMap<AccountId32, WorkerCore>,
    projects: BTreeMap<EntityId, ProjectCore>,
    balances: BTreeMap<AccountId32, Money>,
    reasons: Vec<RecordedReason>,
    disputes: BTreeMap<EntityId, generated_contracts::Dispute>,
    #[serde(default)]
    bramp: bramp::BrampState,
    next_task_id: u32,
    minted_units: Money,
    #[serde(skip)]
    skill_index: BTreeMap<u32, BTreeSet<AccountId32>>,
    #[serde(skip)]
    context: execution::EngineContext,
}

impl State {
    pub(crate) fn new(root_account: AccountId32) -> Result<Self> {
        let shell = execution::Shell::new(root_account)?;
        let generation = shell.info.provider_instance_id;
        Ok(Self {
            shell,
            engine_version: rust_engine_version(),
            engine: RustEngine::new(root_account, generation)?,
        })
    }

    #[cfg(test)]
    fn set_fixture_generation(&mut self, generation: generated_contracts::ProviderInstanceId) {
        use generated_contracts::{
            CatalogKind,
            provider_core::{CatalogReferenceV1, catalog_seed_v1},
        };
        self.shell.info.provider_instance_id = generation;
        for seed in catalog_seed_v1() {
            let reference = CatalogReferenceV1::Seed(crate::migration::source_hash(
                &seed.record(generation).hash_input(),
            ));
            let catalog = if seed.kind == CatalogKind::Role {
                &mut self.engine.roles
            } else {
                &mut self.engine.skills
            };
            if let Some(entry) = catalog.get_mut(&seed.id)
                && matches!(entry.content, CatalogReferenceV1::Seed(_))
            {
                entry.content = reference.clone();
            }
            if seed.kind == CatalogKind::Skill
                && let Some(metadata) = self.engine.skill_metadata.get_mut(&seed.id)
                && matches!(metadata.origin, CatalogReferenceV1::Seed(_))
            {
                metadata.origin = reference;
            }
        }
    }

    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn seed(&mut self) {
        self.engine.seed(self.shell.info.provider_instance_id);
    }

    pub(crate) fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<generated_contracts::provider_core::ReasonPageV1> {
        require((1..=256).contains(&limit), "invalid_reason_limit")?;
        let start = usize::try_from(after).map_err(|_| Error::bad("invalid_reason_cursor"))?;
        require(start <= self.engine.reasons.len(), "invalid_reason_cursor")?;
        let reasons: Vec<_> = self
            .engine
            .reasons
            .iter()
            .skip(start)
            .take(limit)
            .cloned()
            .collect();
        Ok(generated_contracts::provider_core::ReasonPageV1 {
            generation: self.info().provider_instance_id,
            start: after,
            next: after
                .checked_add(u64::try_from(reasons.len()).map_err(|_| Error::internal())?)
                .ok_or_else(Error::internal)?,
            total: u64::try_from(self.engine.reasons.len()).map_err(|_| Error::internal())?,
            reasons,
        })
    }
    pub(crate) fn skill_requests(&self, origin: AccountId32) -> Vec<SkillRequestCore> {
        self.engine.skill_requests(origin)
    }
    pub(crate) fn bramp_deposit(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> Result<generated_contracts::DepositView> {
        self.engine.bramp_deposit(origin, id)
    }
    pub(crate) fn bramp_withdrawal(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> Result<generated_contracts::WithdrawalView> {
        self.engine.bramp_withdrawal(origin, id)
    }
    pub(crate) fn dispute_view(
        &self,
        id: EntityId,
    ) -> Result<generated_contracts::provider_core::CoreDisputeView> {
        self.engine.dispute_view(id)
    }
    pub(crate) fn info(&self) -> ProviderInfo {
        self.shell.info.clone()
    }
    pub(crate) fn nonce(&self, account: AccountId32) -> AccountNonce {
        AccountNonce {
            account,
            nonce: self.shell.nonces.get(&account).copied().unwrap_or(0),
        }
    }
    pub(crate) fn receipt(&self, operation: OperationId) -> Option<OperationReceipt> {
        self.shell
            .receipts
            .get(&operation)
            .map(|record| record.receipt.clone())
    }
    pub(crate) fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<generated_contracts::provider_core::TaskAllocationV1>> {
        let allocation = self.shell.task_allocation(operation)?;
        if allocation
            .as_ref()
            .is_some_and(|value| value.task_id >= self.engine.next_task_id)
        {
            return Err(Error::unavailable());
        }
        Ok(allocation)
    }
    pub(crate) fn task_state(&self) -> generated_contracts::provider_core::TaskStateV2 {
        use generated_contracts::provider_core::{TaskStateV2, TaskStorageRecordV2};
        let mut state =
            TaskStateV2 {
                schema_version: 2,
                has_task_history: self.shell.receipts.values().any(|record| {
                    generated_contracts::UnsignedContractCallV1::decode_signable(&record.bytes)
                            .map_or(true, |call| {
                                matches!(call.command,
                    generated_contracts::ProviderCommand::CreateTask { .. }
                    | generated_contracts::ProviderCommand::EditTask { .. }
                    | generated_contracts::ProviderCommand::UpdateTaskProgress { .. }
                    | generated_contracts::ProviderCommand::DeleteTask { .. }
                    | generated_contracts::ProviderCommand::CreateTaskCore { .. }
                    | generated_contracts::ProviderCommand::SetTaskAssigneesCore { .. })
                            })
                }) || self.shell.events.iter().any(|event| {
                    matches!(
                        event.kind,
                        DomainEventKind::TaskCreated
                            | DomainEventKind::TaskUpdated
                            | DomainEventKind::TaskDeleted
                    )
                }),
                generation: self.info().provider_instance_id,
                next_task_id: self.engine.next_task_id,
                storages: self
                    .engine
                    .projects
                    .values()
                    .flat_map(|project| {
                        project.proposals.iter().flat_map(move |proposal| {
                            proposal
                                .milestones
                                .iter()
                                .map(move |milestone| TaskStorageRecordV2 {
                                    project_id: project.project_id,
                                    proposal_id: proposal.proposal_id,
                                    proposal_revision: proposal.revision,
                                    storage: milestone.task_storage.clone(),
                                })
                        })
                    })
                    .collect(),
            };
        state
            .storages
            .sort_by_key(|record| record.storage.task_storage_id);
        state
    }
    pub(crate) fn snapshot(&self) -> CoreSnapshotV5 {
        CoreSnapshotV5 {
            schema_version: 5,
            info: self.info(),
            catalog: CatalogCore {
                roles: self.engine.roles.values().cloned().collect(),
                skills: self.engine.skills.values().cloned().collect(),
                skill_roles: self
                    .engine
                    .skill_metadata
                    .iter()
                    .map(|(skill_id, metadata)| SkillRoleCore {
                        skill_id: *skill_id,
                        role_ids: metadata.role_ids.clone(),
                        origin: metadata.origin.clone(),
                    })
                    .collect(),
                score_policy: self.engine.score_policy,
                compensation_policy: self.engine.compensation_policy.clone(),
            },
            workers: self.engine.workers.values().cloned().collect(),
            projects: self.engine.projects.values().cloned().collect(),
            balances: self
                .engine
                .balances
                .iter()
                .map(|(account, available)| BalanceView {
                    account: *account,
                    asset_id: 1,
                    available: *available,
                })
                .collect(),
        }
    }
    pub(crate) fn events(&self, after: u64, limit: usize) -> ProviderEvents {
        let events: Vec<_> = self
            .shell
            .events
            .iter()
            .filter(|event| event.cursor > after)
            .take(limit)
            .cloned()
            .collect();
        let next_cursor = events.last().map_or(after, |event| event.cursor);
        ProviderEvents {
            provider_instance_id: self.shell.info.provider_instance_id,
            events,
            next_cursor,
        }
    }

    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn restore(bytes: &[u8], root_account: AccountId32) -> Result<Self> {
        let state: Self = serde_json::from_slice(bytes)
            .map_err(|_| Error::domain("state_configuration_mismatch"))?;
        state.validate_restored(root_account)
    }

    pub(crate) fn archive(&self) -> Result<CoreArchiveV5> {
        self.engine.validate_task_keys()?;
        Ok(CoreArchiveV5 {
            version: 5,
            checkpoint: serde_json::from_slice(&self.encode()?).map_err(|_| Error::internal())?,
        })
    }

    pub(crate) fn from_archive(archive: CoreArchiveV5, root_account: AccountId32) -> Result<Self> {
        require(archive.version == 5, "state_configuration_mismatch")?;
        let state = Self::from(archive.checkpoint).validate_restored(root_account)?;
        state.engine.validate_task_keys()?;
        Ok(state)
    }

    fn validate_restored(mut self, root_account: AccountId32) -> Result<Self> {
        let state = &mut self;
        require(
            state.engine_version == rust_engine_version()
                && state.shell.info.root_account == root_account
                && state.shell.info.payload_version == generated_contracts::PAYLOAD_VERSION,
            "state_configuration_mismatch",
        )?;
        // Older snapshots retain the real creation time in their durable events.
        for event in &state.shell.events {
            if event.kind == DomainEventKind::ProjectCreated
                && let Some(id) = event.project_id
                && let Some(project) = state.engine.projects.get_mut(&id)
            {
                project.created_at = project.created_at.or(Some(event.occurred_at));
            }
        }
        state.engine.context = execution::EngineContext::new(root_account);
        state.engine.context.event_cursor =
            u64::try_from(state.shell.events.len()).map_err(|_| Error::internal())?;
        state.engine.validate()?;
        state.validate_history()?;
        require(
            state.shell.receipts.values().all(|record| {
                record
                    .allocated_task_id
                    .is_none_or(|id| id < state.engine.next_task_id)
            }),
            "invalid_task_allocation_state",
        )?;
        state.engine.rebuild_index();
        Ok(self)
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| Error::internal())
    }
}

fn empty_score() -> ReputationView {
    ReputationView {
        weighted_score_sum: 0,
        rated_minutes: 0,
    }
}
fn validate_score(score: &ReputationView) -> Result<()> {
    require(
        score.weighted_score_sum <= u128::from(score.rated_minutes) * 500,
        "invalid_score_state",
    )
}
fn add_rating(score: &mut ReputationView, hundredths: u16, minutes: Minutes) -> Result<()> {
    add_weighted_rating(score, hundredths, minutes, 1)
}
fn add_weighted_rating(
    score: &mut ReputationView,
    hundredths: u16,
    minutes: Minutes,
    weight: u8,
) -> Result<()> {
    let weighted_minutes = u64::from(minutes.get()) * u64::from(weight);
    score.rated_minutes = score
        .rated_minutes
        .checked_add(weighted_minutes)
        .ok_or_else(|| Error::domain("score_overflow"))?;
    score.weighted_score_sum = score
        .weighted_score_sum
        .checked_add(u128::from(hundredths) * u128::from(weighted_minutes))
        .ok_or_else(|| Error::domain("score_overflow"))?;
    validate_score(score)
}
fn score_fraction(score: &ReputationView) -> (u128, u128) {
    if score.rated_minutes == 0 {
        (250, 1)
    } else {
        (score.weighted_score_sum, u128::from(score.rated_minutes))
    }
}
/// Continued fractions compare exact ratios without overflowing cross-products.
fn compare_scores(left: &ReputationView, right: &ReputationView) -> Ordering {
    let (mut ln, mut ld) = score_fraction(left);
    let (mut rn, mut rd) = score_fraction(right);
    let mut inverted = false;
    loop {
        let mut order = (ln / ld).cmp(&(rn / rd));
        if order != Ordering::Equal {
            return if inverted { order.reverse() } else { order };
        }
        let (lr, rr) = (ln % ld, rn % rd);
        if lr == 0 || rr == 0 {
            order = lr.cmp(&rr);
            return if inverted { order.reverse() } else { order };
        }
        (ln, ld, rn, rd) = (ld, lr, rd, rr);
        inverted = !inverted;
    }
}

impl RustEngine {
    fn new(
        root_account: AccountId32,
        generation: generated_contracts::ProviderInstanceId,
    ) -> Result<Self> {
        let mut state = Self {
            roles: BTreeMap::new(),
            skills: BTreeMap::new(),
            skill_metadata: BTreeMap::new(),
            skill_requests: BTreeMap::new(),
            score_policy: ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?,
            compensation_policy: generated_contracts::CompensationPolicy::default(),
            workers: BTreeMap::new(),
            projects: BTreeMap::new(),
            balances: BTreeMap::new(),
            reasons: Vec::new(),
            disputes: BTreeMap::new(),
            bramp: bramp::BrampState::default(),
            next_task_id: 1,
            minted_units: Money::ZERO,
            skill_index: BTreeMap::new(),
            context: execution::EngineContext::new(root_account),
        };
        state.roles.insert(1, crate::seed::coordinator(generation));
        state.seed(generation);
        Ok(state)
    }

    pub(crate) fn seed(&mut self, generation: generated_contracts::ProviderInstanceId) {
        #[cfg(feature = "mock-seed")]
        crate::seed::seed(
            generation,
            &mut self.roles,
            &mut self.skills,
            &mut self.skill_metadata,
        );
        #[cfg(not(feature = "mock-seed"))]
        let _ = generation;
    }

    fn rebuild_index(&mut self) {
        self.skill_index.clear();
        for worker in self.workers.values() {
            for skill in &worker.qualifications.skill_ids {
                self.skill_index
                    .entry(*skill)
                    .or_default()
                    .insert(worker.account);
            }
        }
    }
}

const fn rust_engine_version() -> u32 {
    5
}
