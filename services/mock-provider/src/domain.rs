use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use generated_contracts::{
    AccountId32, AccountNonce, AssignmentView, BalanceView, CalendarView, CatalogEntry,
    CatalogKind, CatalogView, DomainEvent, DomainEventKind, EntityId, ExecutionOutcome,
    MilestoneStatus, MilestoneView, Minutes, Money, OperationId, OperationReceipt, PAYLOAD_VERSION,
    Percentage, PlanningStatus, PlanningView, ProjectView, ProposalDefinition, ProposalStatus,
    ProposalView, ProviderCommand, ProviderEvents, ProviderInfo, ProviderInstanceId,
    ProviderSnapshot, Qualifications, ReputationView, ReservationView, ScorePolicy,
    SignedContractCallV1, TaskDefinition, TaskStorageView, TaskView, TeamRating, UnixSeconds,
    UnsignedContractCallV1, WorkerMode, WorkerView,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use subxt_signer::sr25519;

use crate::{Error, Result, calendar, random_id, require, seed::SkillMetadata};

#[derive(Clone, Serialize, Deserialize)]
struct RecordedCall {
    bytes: Vec<u8>,
    receipt: OperationReceipt,
}

#[derive(Clone, Serialize, Deserialize)]
struct RecordedReason {
    project_id: EntityId,
    milestone_id: Option<EntityId>,
    origin: AccountId32,
    reason: String,
    occurred_at: UnixSeconds,
    kind: DomainEventKind,
}

/// Private aggregate: only authenticated calls can obtain a transition and only
/// storage can commit it. HTTP snapshots are copies, never mutable handles.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct State {
    info: ProviderInfo,
    roles: BTreeMap<u32, CatalogEntry>,
    skills: BTreeMap<u32, CatalogEntry>,
    skill_metadata: BTreeMap<u32, SkillMetadata>,
    score_policy: ScorePolicy,
    workers: BTreeMap<AccountId32, WorkerView>,
    projects: BTreeMap<EntityId, ProjectView>,
    balances: BTreeMap<AccountId32, Money>,
    nonces: BTreeMap<AccountId32, u64>,
    receipts: BTreeMap<OperationId, RecordedCall>,
    events: Vec<DomainEvent>,
    reasons: Vec<RecordedReason>,
    next_task_id: u32,
    minted_units: Money,
    #[serde(skip)]
    skill_index: BTreeMap<u32, BTreeSet<AccountId32>>,
}

/// Typestate gate: unverified envelopes cannot be handed to the state engine.
pub(crate) struct VerifiedCall {
    call: UnsignedContractCallV1,
    bytes: Vec<u8>,
}
impl VerifiedCall {
    pub(crate) fn verify(signed: SignedContractCallV1) -> Result<Self> {
        let bytes = signed.call.signable_bytes()?;
        let public = sr25519::PublicKey(signed.call.origin.into_bytes());
        let signature = sr25519::Signature(signed.signature.into_bytes());
        require(
            sr25519::verify(&signature, &bytes, &public),
            "invalid_signature",
        )?;
        Ok(Self {
            call: signed.call,
            bytes,
        })
    }
}

struct Effect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: BTreeSet<AccountId32>,
}
impl Effect {
    fn new(kind: DomainEventKind, entity_id: Option<EntityId>, recipient: AccountId32) -> Self {
        Self {
            kind,
            entity_id,
            recipients: BTreeSet::from([recipient]),
        }
    }
}

impl State {
    pub(crate) fn new(root_account: AccountId32) -> Result<Self> {
        let mut state = Self {
            info: ProviderInfo {
                provider_instance_id: ProviderInstanceId::from_bytes(random_id()?.into_bytes()),
                root_account,
                payload_version: PAYLOAD_VERSION,
            },
            roles: BTreeMap::new(),
            skills: BTreeMap::new(),
            skill_metadata: BTreeMap::new(),
            score_policy: ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?,
            workers: BTreeMap::new(),
            projects: BTreeMap::new(),
            balances: BTreeMap::new(),
            nonces: BTreeMap::new(),
            receipts: BTreeMap::new(),
            events: Vec::new(),
            reasons: Vec::new(),
            next_task_id: 1,
            minted_units: Money::ZERO,
            skill_index: BTreeMap::new(),
        };
        // The protected coordinator identity exists even without optional seed data.
        state.roles.insert(
            1,
            CatalogEntry {
                id: 1,
                name: "coordinator".into(),
                fixed: true,
            },
        );
        state.seed();
        Ok(state)
    }

    pub(crate) fn seed(&mut self) {
        #[cfg(feature = "mock-seed")]
        crate::seed::seed(&mut self.roles, &mut self.skills, &mut self.skill_metadata);
    }

    pub(crate) fn info(&self) -> ProviderInfo {
        self.info.clone()
    }
    pub(crate) fn nonce(&self, account: AccountId32) -> AccountNonce {
        AccountNonce {
            account,
            nonce: self.nonces.get(&account).copied().unwrap_or(0),
        }
    }
    pub(crate) fn receipt(&self, operation: OperationId) -> Option<OperationReceipt> {
        self.receipts
            .get(&operation)
            .map(|record| record.receipt.clone())
    }
    pub(crate) fn snapshot(&self) -> ProviderSnapshot {
        ProviderSnapshot {
            info: self.info(),
            catalog: CatalogView {
                roles: self.roles.values().cloned().collect(),
                skills: self.skills.values().cloned().collect(),
                score_policy: self.score_policy,
            },
            workers: self.workers.values().cloned().collect(),
            projects: self.projects.values().cloned().collect(),
            balances: self
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
            .events
            .iter()
            .filter(|event| event.cursor > after)
            .take(limit)
            .cloned()
            .collect();
        let next_cursor = events.last().map_or(after, |event| event.cursor);
        ProviderEvents {
            provider_instance_id: self.info.provider_instance_id,
            events,
            next_cursor,
        }
    }

    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn restore(bytes: &[u8], root_account: AccountId32) -> Result<Self> {
        let mut state: Self = serde_json::from_slice(bytes).map_err(|_| Error::internal())?;
        require(
            state.info.root_account == root_account
                && state.info.payload_version == PAYLOAD_VERSION,
            "state_configuration_mismatch",
        )?;
        state.validate()?;
        state.rebuild_index();
        Ok(state)
    }
    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| Error::internal())
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

    #[allow(clippy::too_many_lines)] // Validate the complete persisted aggregate at its boundary.
    fn validate(&self) -> Result<()> {
        require(
            self.roles
                .get(&1)
                .is_some_and(|role| role.name == "coordinator" && role.fixed),
            "invalid_catalog_state",
        )?;
        let mut identities = BTreeSet::new();
        for (account, worker) in &self.workers {
            require(
                *account == worker.account
                    && worker.calendar.owner == *account
                    && identities.insert(worker.calendar.calendar_id),
                "invalid_worker_state",
            )?;
            require(
                worker.mode != WorkerMode::Coordinator || worker.coordinator_eligible,
                "invalid_worker_state",
            )?;
            self.qualifications(&worker.qualifications)?;
            calendar::validate(&worker.calendar)?;
            validate_score(&worker.worker_score)?;
            validate_score(&worker.coordinator_score)?;
        }
        let mut total = self
            .balances
            .values()
            .try_fold(Money::ZERO, |sum, amount| sum.checked_add(*amount))?;
        for (id, project) in &self.projects {
            require(
                *id == project.project_id && identities.insert(*id),
                "invalid_project_state",
            )?;
            require(
                self.workers.contains_key(&project.coordinator),
                "invalid_coordinator_state",
            )?;
            total = total
                .checked_add(project.planning.escrow)?
                .checked_add(project.execution_escrow)?;
            let mut expected_execution = Money::ZERO;
            for proposal in &project.proposals {
                require(identities.insert(proposal.proposal_id), "duplicate_entity")?;
                proposal_definition(proposal).validate()?;
                for milestone in &proposal.milestones {
                    require(
                        identities.insert(milestone.milestone_id)
                            && identities.insert(milestone.task_storage.task_storage_id)
                            && milestone.task_storage.milestone_id == milestone.milestone_id,
                        "invalid_storage_state",
                    )?;
                    for task in &milestone.task_storage.tasks {
                        task.task.validate()?;
                    }
                    if milestone.status.is_some() {
                        require(
                            milestone.assignments.len() == milestone.definition.requirements.len(),
                            "invalid_assignment_state",
                        )?;
                        require(
                            milestone
                                .assignments
                                .iter()
                                .map(|assignment| assignment.worker)
                                .collect::<BTreeSet<_>>()
                                .len()
                                == milestone.assignments.len(),
                            "invalid_assignment_state",
                        )?;
                        for requirement in &milestone.definition.requirements {
                            require(
                                milestone.assignments.iter().any(|assignment| {
                                    assignment.requirement_key == requirement.key
                                        && self.workers.contains_key(&assignment.worker)
                                }),
                                "invalid_assignment_state",
                            )?;
                        }
                        if milestone.status != Some(MilestoneStatus::Completed) {
                            expected_execution =
                                expected_execution.checked_add(milestone.definition.total()?)?;
                        }
                    } else {
                        require(milestone.assignments.is_empty(), "invalid_assignment_state")?;
                    }
                }
            }
            require(
                project.execution_escrow == expected_execution,
                "escrow_mismatch",
            )?;
            let planning_fee = project
                .planning
                .quote
                .as_ref()
                .map_or(Money::ZERO, |quote| quote.fee);
            match project.planning.status {
                PlanningStatus::AwaitingQuote
                | PlanningStatus::Quoted
                | PlanningStatus::Completed => {
                    require(project.planning.escrow == Money::ZERO, "escrow_mismatch")?;
                }
                PlanningStatus::Accepted | PlanningStatus::Delivered | PlanningStatus::Disputed => {
                    require(project.planning.escrow == planning_fee, "escrow_mismatch")?;
                }
            }
        }
        require(total == self.minted_units, "supply_mismatch")?;
        for (index, event) in self.events.iter().enumerate() {
            require(
                event.provider_instance_id == self.info.provider_instance_id
                    && event.cursor == u64::try_from(index).map_err(|_| Error::internal())? + 1,
                "invalid_event_state",
            )?;
        }
        for (operation_id, recorded) in &self.receipts {
            let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
            require(
                call.operation_id == *operation_id
                    && recorded.receipt.operation_id == *operation_id
                    && call.provider_instance_id == self.info.provider_instance_id
                    && recorded.receipt.origin == call.origin
                    && recorded.receipt.nonce == call.nonce
                    && self.nonce(call.origin).nonce > call.nonce,
                "invalid_receipt_state",
            )?;
        }
        Ok(())
    }

    /// Failed domain execution commits only its nonce and failed receipt; its
    /// tentative business state and events are discarded as a unit.
    pub(crate) fn execute(
        mut self,
        verified: VerifiedCall,
        now: UnixSeconds,
    ) -> Result<(Self, OperationReceipt)> {
        let VerifiedCall { call, bytes } = verified;
        require(
            call.provider_instance_id == self.info.provider_instance_id,
            "provider_instance_mismatch",
        )?;
        if let Some(record) = self.receipts.get(&call.operation_id) {
            require(record.bytes == bytes, "idempotency_conflict")?;
            let receipt = record.receipt.clone();
            return Ok((self, receipt));
        }
        require(call.expires_at > now, "expired_call")?;
        require(
            call.nonce == self.nonce(call.origin).nonce,
            "nonce_conflict",
        )?;
        let next_nonce = call
            .nonce
            .checked_add(1)
            .ok_or_else(|| Error::domain("nonce_exhausted"))?;
        let mut tentative = self.clone();
        let transition = tentative.apply(call.origin, &call.command, now);
        let (outcome, entity_id, cursor) = match transition {
            Ok(effect) => {
                tentative.validate()?;
                let cursor = u64::try_from(tentative.events.len())
                    .map_err(|_| Error::internal())?
                    .checked_add(1)
                    .ok_or_else(Error::internal)?;
                tentative.events.push(DomainEvent {
                    provider_instance_id: self.info.provider_instance_id,
                    cursor,
                    operation_id: call.operation_id,
                    kind: effect.kind,
                    project_id: call.command.project_id().or({
                        if matches!(call.command, ProviderCommand::CreateProject(_)) {
                            effect.entity_id
                        } else {
                            None
                        }
                    }),
                    entity_id: effect.entity_id,
                    recipients: effect.recipients.into_iter().collect(),
                    occurred_at: now,
                });
                self = tentative;
                (ExecutionOutcome::Success, effect.entity_id, Some(cursor))
            }
            Err(error) if error.status == axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(error);
            }
            Err(error) => (ExecutionOutcome::Failed(error.code().into()), None, None),
        };
        let receipt = OperationReceipt {
            operation_id: call.operation_id,
            provider_instance_id: self.info.provider_instance_id,
            origin: call.origin,
            nonce: call.nonce,
            outcome,
            created_entity_id: entity_id,
            first_event_cursor: cursor,
            last_event_cursor: cursor,
            finalized_at: now,
        };
        self.nonces.insert(call.origin, next_nonce);
        self.receipts.insert(
            call.operation_id,
            RecordedCall {
                bytes,
                receipt: receipt.clone(),
            },
        );
        Ok((self, receipt))
    }

    fn root(&self, origin: AccountId32) -> Result<()> {
        require(origin == self.info.root_account, "system_origin_required")
    }
    fn qualifications(&self, qualifications: &Qualifications) -> Result<()> {
        qualifications.validate()?;
        require(
            qualifications
                .role_ids
                .iter()
                .all(|id| self.roles.contains_key(id))
                && qualifications
                    .skill_ids
                    .iter()
                    .all(|id| self.skills.contains_key(id)),
            "unknown_qualification",
        )
    }
    fn proposal_qualifications(&self, proposal: &ProposalDefinition) -> Result<()> {
        for milestone in &proposal.milestones {
            for requirement in &milestone.requirements {
                self.qualifications(&Qualifications {
                    role_ids: vec![requirement.role_id],
                    skill_ids: requirement.skill_ids.clone(),
                })?;
            }
        }
        Ok(())
    }
    fn worker_mut(&mut self, account: AccountId32) -> Result<&mut WorkerView> {
        self.workers
            .get_mut(&account)
            .ok_or_else(|| Error::domain("worker_not_found"))
    }
    fn credit(&mut self, account: AccountId32, amount: Money) -> Result<()> {
        let balance = self.balances.entry(account).or_default();
        *balance = balance.checked_add(amount)?;
        Ok(())
    }
    fn debit(&mut self, account: AccountId32, amount: Money) -> Result<()> {
        let balance = self.balances.entry(account).or_default();
        require(*balance >= amount, "insufficient_balance")?;
        *balance = balance.checked_sub(amount)?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Closed command dispatcher keeps the authorization boundary visible.
    fn apply(
        &mut self,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        let effect = match command {
            ProviderCommand::RegisterWorker(request) => {
                require(!self.workers.contains_key(&origin), "worker_exists")?;
                self.qualifications(&request.qualifications)?;
                let calendar_id = random_id()?;
                self.workers.insert(
                    origin,
                    WorkerView {
                        account: origin,
                        display_name: request.display_name.clone(),
                        qualifications: request.qualifications.clone(),
                        mode: WorkerMode::Worker,
                        coordinator_eligible: false,
                        calendar: CalendarView {
                            calendar_id,
                            owner: origin,
                            definition: request.calendar.clone(),
                            reservations: Vec::new(),
                        },
                        worker_score: empty_score(),
                        coordinator_score: empty_score(),
                    },
                );
                self.rebuild_index();
                Effect::new(DomainEventKind::WorkerRegistered, Some(calendar_id), origin)
            }
            ProviderCommand::UpdateQualifications(request) => {
                self.qualifications(&request.qualifications)?;
                self.worker_mut(origin)?.qualifications = request.qualifications.clone();
                self.rebuild_index();
                Effect::new(DomainEventKind::WorkerUpdated, None, origin)
            }
            ProviderCommand::SetCalendar(definition) => {
                let worker = self.worker_mut(origin)?;
                worker.calendar.definition = definition.clone();
                calendar::validate(&worker.calendar)?;
                Effect::new(
                    DomainEventKind::CalendarUpdated,
                    Some(worker.calendar.calendar_id),
                    origin,
                )
            }
            ProviderCommand::SetWorkerMode(request) => {
                let worker = self.worker_mut(origin)?;
                require(
                    request.mode != WorkerMode::Coordinator || worker.coordinator_eligible,
                    "coordinator_not_eligible",
                )?;
                worker.mode = request.mode;
                Effect::new(DomainEventKind::WorkerUpdated, None, origin)
            }
            ProviderCommand::PromoteCoordinator(request) => {
                self.root(origin)?;
                self.worker_mut(request.account)?.coordinator_eligible = true;
                Effect::new(DomainEventKind::CoordinatorPromoted, None, request.account)
            }
            ProviderCommand::UpsertCatalogEntry(request) => {
                self.root(origin)?;
                require(
                    !(request.kind == CatalogKind::Role && request.id == 1),
                    "fixed_coordinator_role",
                )?;
                let catalog = if request.kind == CatalogKind::Role {
                    &mut self.roles
                } else {
                    &mut self.skills
                };
                catalog.insert(
                    request.id,
                    CatalogEntry {
                        id: request.id,
                        name: request.name.clone(),
                        fixed: false,
                    },
                );
                Effect::new(DomainEventKind::CatalogUpdated, None, origin)
            }
            ProviderCommand::DeleteCatalogEntry(request) => {
                self.root(origin)?;
                require(
                    !(request.kind == CatalogKind::Role && request.id == 1),
                    "fixed_coordinator_role",
                )?;
                let references_worker = self.workers.values().any(|worker| {
                    if request.kind == CatalogKind::Role {
                        worker.qualifications.role_ids.contains(&request.id)
                    } else {
                        worker.qualifications.skill_ids.contains(&request.id)
                    }
                });
                let references_proposal = self
                    .projects
                    .values()
                    .flat_map(|project| &project.proposals)
                    .flat_map(|proposal| &proposal.milestones)
                    .flat_map(|milestone| &milestone.definition.requirements)
                    .any(|requirement| {
                        if request.kind == CatalogKind::Role {
                            requirement.role_id == request.id
                        } else {
                            requirement.skill_ids.contains(&request.id)
                        }
                    });
                require(
                    !references_worker && !references_proposal,
                    "catalog_entry_in_use",
                )?;
                let catalog = if request.kind == CatalogKind::Role {
                    &mut self.roles
                } else {
                    &mut self.skills
                };
                require(
                    catalog.remove(&request.id).is_some(),
                    "catalog_entry_not_found",
                )?;
                if request.kind == CatalogKind::Skill {
                    self.skill_metadata.remove(&request.id);
                }
                Effect::new(DomainEventKind::CatalogUpdated, None, origin)
            }
            ProviderCommand::FundAccount(request) => {
                self.root(origin)?;
                require(cfg!(feature = "mock-seed"), "mock_funding_disabled")?;
                self.minted_units = self.minted_units.checked_add(request.amount)?;
                self.credit(request.account, request.amount)?;
                Effect::new(DomainEventKind::AccountFunded, None, request.account)
            }
            ProviderCommand::SetScorePolicy(policy) => {
                self.root(origin)?;
                self.score_policy = *policy;
                Effect::new(DomainEventKind::ScorePolicyUpdated, None, origin)
            }
            ProviderCommand::CreateProject(request) => {
                // Initial selection has no quoted window yet: this indicative
                // filter uses the current week. Quote acceptance reserves the
                // actual explicit window against fresh capacity atomically.
                let week = calendar::current_week(now)?;
                let mut candidates = Vec::new();
                for worker in self.workers.values().filter(|worker| {
                    worker.mode == WorkerMode::Coordinator
                        && worker.coordinator_eligible
                        && worker.account != origin
                }) {
                    if calendar::available(&worker.calendar, week)? != Minutes::ZERO {
                        candidates.push(worker.account);
                    }
                }
                let coordinator = self.select(&candidates, WorkerMode::Coordinator)?;
                let project_id = random_id()?;
                self.projects.insert(
                    project_id,
                    ProjectView {
                        project_id,
                        client: origin,
                        coordinator,
                        title: request.title.clone(),
                        description: request.description.clone(),
                        planning: PlanningView {
                            revision: 0,
                            status: PlanningStatus::AwaitingQuote,
                            quote: None,
                            escrow: Money::ZERO,
                            frozen: false,
                        },
                        proposals: Vec::new(),
                        execution_escrow: Money::ZERO,
                        cancelled: false,
                    },
                );
                let mut effect =
                    Effect::new(DomainEventKind::ProjectCreated, Some(project_id), origin);
                effect.recipients.insert(coordinator);
                effect
            }
            _ => {
                let project_id = command
                    .project_id()
                    .ok_or_else(|| Error::bad("invalid_target"))?;
                let mut project = self
                    .projects
                    .remove(&project_id)
                    .ok_or_else(|| Error::domain("project_not_found"))?;
                require(!project.cancelled, "project_cancelled")?;
                let mut effect = self.project_command(&mut project, origin, command, now)?;
                effect.recipients.insert(project.client);
                effect.recipients.insert(project.coordinator);
                for milestone in project
                    .proposals
                    .iter()
                    .flat_map(|proposal| &proposal.milestones)
                {
                    effect.recipients.extend(
                        milestone
                            .assignments
                            .iter()
                            .map(|assignment| assignment.worker),
                    );
                    effect.recipients.extend(
                        milestone
                            .task_storage
                            .tasks
                            .iter()
                            .flat_map(|task| task.task.assignees.iter())
                            .copied(),
                    );
                }
                self.projects.insert(project_id, project);
                effect
            }
        };
        Ok(effect)
    }

    fn select(&self, candidates: &[AccountId32], mode: WorkerMode) -> Result<AccountId32> {
        let mut winners = Vec::new();
        let mut best: Option<&ReputationView> = None;
        for account in candidates {
            let worker = self.workers.get(account).ok_or_else(Error::internal)?;
            let score = if mode == WorkerMode::Worker {
                &worker.worker_score
            } else {
                &worker.coordinator_score
            };
            let comparison =
                best.map_or(Ordering::Greater, |current| compare_scores(score, current));
            if comparison == Ordering::Greater {
                winners.clear();
                best = Some(score);
            }
            if comparison != Ordering::Less {
                winners.push(*account);
            }
        }
        require(!winners.is_empty(), "no_available_candidate")?;
        let index = if winners.len() == 1 {
            0
        } else {
            rand::rng().random_range(0..winners.len())
        };
        Ok(winners[index])
    }

    fn assign(&mut self, project: &ProjectView, milestone: &mut MilestoneView) -> Result<()> {
        let window = milestone.definition.window;
        let reference = ReservationView {
            project_id: project.project_id,
            milestone_id: Some(milestone.milestone_id),
            requirement_key: None,
            week: window.start(),
            minutes: Minutes::ZERO,
        };
        let coordinator = self.worker_mut(project.coordinator)?;
        calendar::reserve(
            &mut coordinator.calendar,
            window,
            milestone.definition.coordinator_minutes,
            &reference,
        )?;
        let mut used = BTreeSet::from([project.client, project.coordinator]);
        for requirement in &milestone.definition.requirements {
            let mut skills = requirement.skill_ids.iter();
            let first = skills.next().ok_or_else(|| Error::bad("missing_skills"))?;
            let mut candidates = self.skill_index.get(first).cloned().unwrap_or_default();
            for skill in skills {
                candidates.retain(|account| {
                    self.skill_index
                        .get(skill)
                        .is_some_and(|indexed| indexed.contains(account))
                });
            }
            let mut available = Vec::new();
            for account in candidates {
                let worker = self.workers.get(&account).ok_or_else(Error::internal)?;
                if used.contains(&account) || worker.mode != WorkerMode::Worker {
                    continue;
                }
                match calendar::allocation(&worker.calendar, window, requirement.minutes) {
                    Ok(_) => available.push(account),
                    Err(error) if error.code() == "insufficient_capacity" => {}
                    Err(error) => return Err(error),
                }
            }
            let account = self.select(&available, WorkerMode::Worker)?;
            calendar::reserve(
                &mut self.worker_mut(account)?.calendar,
                window,
                requirement.minutes,
                &ReservationView {
                    requirement_key: Some(requirement.key),
                    ..reference.clone()
                },
            )?;
            milestone.assignments.push(AssignmentView {
                requirement_key: requirement.key,
                worker: account,
            });
            used.insert(account);
        }
        milestone.status = Some(MilestoneStatus::InProgress);
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Exhaustive messages execute inside one storage transaction.
    fn project_command(
        &mut self,
        project: &mut ProjectView,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        let project_id = project.project_id;
        let mut entity = Some(project_id);
        let kind = match command {
            ProviderCommand::QuotePlanning { quote, .. } => {
                coordinator(project, origin)?;
                require(
                    matches!(
                        project.planning.status,
                        PlanningStatus::AwaitingQuote | PlanningStatus::Quoted
                    ),
                    "invalid_planning_state",
                )?;
                project.planning.quote = Some(quote.clone());
                project.planning.status = PlanningStatus::Quoted;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningQuoted
            }
            ProviderCommand::AcceptPlanningQuote {
                expected_revision, ..
            } => {
                client(project, origin)?;
                revision(project.planning.revision, *expected_revision)?;
                require(
                    project.planning.status == PlanningStatus::Quoted && !project.planning.frozen,
                    "invalid_planning_state",
                )?;
                let quote = project
                    .planning
                    .quote
                    .as_ref()
                    .ok_or_else(Error::internal)?;
                self.debit(project.client, quote.fee)?;
                calendar::reserve(
                    &mut self.worker_mut(project.coordinator)?.calendar,
                    quote.window,
                    quote.minutes,
                    &ReservationView {
                        project_id,
                        milestone_id: None,
                        requirement_key: None,
                        week: quote.window.start(),
                        minutes: Minutes::ZERO,
                    },
                )?;
                project.planning.escrow = quote.fee;
                project.planning.status = PlanningStatus::Accepted;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningAccepted
            }
            ProviderCommand::CreateProposal { proposal, .. } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let proposal_id = random_id()?;
                project.proposals.push(ProposalView {
                    proposal_id,
                    revision: 0,
                    title: proposal.title.clone(),
                    description: proposal.description.clone(),
                    status: ProposalStatus::Draft,
                    milestones: make_milestones(proposal, &[])?,
                    change_request: None,
                });
                entity = Some(proposal_id);
                DomainEventKind::ProposalCreated
            }
            ProviderCommand::UpdateProposal {
                proposal_id,
                proposal,
                ..
            } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                self.proposal_qualifications(proposal)?;
                let current = proposal_mut(project, *proposal_id)?;
                require(
                    current.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                current.milestones = make_milestones(proposal, &current.milestones)?;
                current.title.clone_from(&proposal.title);
                current.description.clone_from(&proposal.description);
                bump(&mut current.revision)?;
                entity = Some(*proposal_id);
                DomainEventKind::ProposalUpdated
            }
            ProviderCommand::DeleteProposal { proposal_id, .. } => {
                coordinator(project, origin)?;
                require(
                    proposal_mut(project, *proposal_id)?.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                project
                    .proposals
                    .retain(|proposal| proposal.proposal_id != *proposal_id);
                entity = Some(*proposal_id);
                DomainEventKind::ProposalDeleted
            }
            ProviderCommand::SubmitProposal { proposal_id, .. } => {
                coordinator(project, origin)?;
                planning_active(project)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::Draft,
                    "proposal_not_draft",
                )?;
                proposal.status = ProposalStatus::PendingApproval;
                bump(&mut proposal.revision)?;
                if project.planning.status != PlanningStatus::Completed {
                    project.planning.status = PlanningStatus::Delivered;
                }
                bump(&mut project.planning.revision)?;
                entity = Some(*proposal_id);
                DomainEventKind::ProposalSubmitted
            }
            ProviderCommand::AcceptPlanningDelivery {
                expected_revision, ..
            } => {
                client(project, origin)?;
                revision(project.planning.revision, *expected_revision)?;
                require(
                    project.planning.status == PlanningStatus::Delivered
                        && !project.planning.frozen,
                    "invalid_planning_state",
                )?;
                self.credit(project.coordinator, project.planning.escrow)?;
                project.planning.escrow = Money::ZERO;
                project.planning.status = PlanningStatus::Completed;
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningCompleted
            }
            ProviderCommand::ApproveExecution {
                proposal_id,
                expected_revision,
                ..
            } => {
                client(project, origin)?;
                require(
                    project.planning.status == PlanningStatus::Completed
                        && !project.planning.frozen,
                    "planning_not_accepted",
                )?;
                require(
                    !project
                        .proposals
                        .iter()
                        .any(|proposal| proposal.status == ProposalStatus::Approved),
                    "execution_already_approved",
                )?;
                let index = project
                    .proposals
                    .iter()
                    .position(|proposal| proposal.proposal_id == *proposal_id)
                    .ok_or_else(|| Error::domain("proposal_not_found"))?;
                let mut proposal = project.proposals[index].clone();
                revision(proposal.revision, *expected_revision)?;
                require(
                    proposal.status == ProposalStatus::PendingApproval,
                    "proposal_not_pending",
                )?;
                let total = proposal_definition(&proposal).total()?;
                self.debit(project.client, total)?;
                for milestone in &mut proposal.milestones {
                    self.assign(project, milestone)?;
                }
                proposal.status = ProposalStatus::Approved;
                bump(&mut proposal.revision)?;
                project.proposals[index] = proposal;
                project.execution_escrow = total;
                entity = Some(*proposal_id);
                DomainEventKind::ExecutionApproved
            }
            ProviderCommand::RequestProposalChanges {
                proposal_id,
                request,
                ..
            } => {
                client(project, origin)?;
                let proposal = proposal_mut(project, *proposal_id)?;
                require(
                    proposal.status == ProposalStatus::PendingApproval,
                    "proposal_not_pending",
                )?;
                proposal.status = ProposalStatus::Draft;
                proposal.change_request = Some(request.reference.clone());
                bump(&mut proposal.revision)?;
                if project.planning.status == PlanningStatus::Delivered {
                    project.planning.status = PlanningStatus::Accepted;
                }
                bump(&mut project.planning.revision)?;
                entity = Some(*proposal_id);
                DomainEventKind::ProposalChangesRequested
            }
            ProviderCommand::CancelProject { request, .. } => {
                participant(project, origin)?;
                self.reasons.push(RecordedReason {
                    project_id,
                    milestone_id: None,
                    origin,
                    reason: request.reason.clone(),
                    occurred_at: now,
                    kind: DomainEventKind::ProjectCancelled,
                });
                project.cancelled = true;
                project.planning.frozen = project.planning.escrow != Money::ZERO;
                bump(&mut project.planning.revision)?;
                for proposal in &mut project.proposals {
                    proposal.status = ProposalStatus::Cancelled;
                    bump(&mut proposal.revision)?;
                    for milestone in &mut proposal.milestones {
                        milestone.frozen = milestone.status != Some(MilestoneStatus::Completed);
                    }
                }
                DomainEventKind::ProjectCancelled
            }
            ProviderCommand::DisputePlanning { request, .. } => {
                participant(project, origin)?;
                require(
                    matches!(
                        project.planning.status,
                        PlanningStatus::Accepted | PlanningStatus::Delivered
                    ),
                    "invalid_planning_state",
                )?;
                project.planning.status = PlanningStatus::Disputed;
                project.planning.frozen = true;
                self.reasons.push(RecordedReason {
                    project_id,
                    milestone_id: None,
                    origin,
                    reason: request.reason.clone(),
                    occurred_at: now,
                    kind: DomainEventKind::PlanningDisputed,
                });
                bump(&mut project.planning.revision)?;
                DomainEventKind::PlanningDisputed
            }
            ProviderCommand::DisputeMilestone {
                milestone_id,
                request,
                ..
            } => {
                participant(project, origin)?;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    matches!(
                        milestone.status,
                        Some(MilestoneStatus::InProgress | MilestoneStatus::CompletionRequested)
                    ),
                    "invalid_milestone_state",
                )?;
                milestone.status = Some(MilestoneStatus::Disputed);
                milestone.frozen = true;
                self.reasons.push(RecordedReason {
                    project_id,
                    milestone_id: Some(*milestone_id),
                    origin,
                    reason: request.reason.clone(),
                    occurred_at: now,
                    kind: DomainEventKind::MilestoneDisputed,
                });
                entity = Some(*milestone_id);
                DomainEventKind::MilestoneDisputed
            }
            ProviderCommand::CreateTask {
                task_storage_id,
                task,
                ..
            } => {
                coordinator(project, origin)?;
                self.task_assignees(task)?;
                let task_id = self.next_task_id;
                self.next_task_id = task_id
                    .checked_add(1)
                    .ok_or_else(|| Error::domain("task_keys_exhausted"))?;
                storage_mut(project, *task_storage_id)?
                    .tasks
                    .push(TaskView {
                        task_id,
                        reporter: origin,
                        created_at: now,
                        updated_at: now,
                        task: task.clone(),
                    });
                entity = Some(*task_storage_id);
                DomainEventKind::TaskCreated
            }
            ProviderCommand::EditTask {
                task_storage_id,
                task_id,
                task,
                ..
            } => {
                coordinator(project, origin)?;
                self.task_assignees(task)?;
                let current = task_mut(project, *task_storage_id, *task_id)?;
                current.task = task.clone();
                current.updated_at = now;
                entity = Some(*task_storage_id);
                DomainEventKind::TaskUpdated
            }
            ProviderCommand::UpdateTaskProgress {
                task_storage_id,
                task_id,
                progress,
                ..
            } => {
                require(origin != project.client, "client_read_only")?;
                let is_coordinator = project.coordinator == origin;
                let current = task_mut(project, *task_storage_id, *task_id)?;
                require(
                    is_coordinator || current.task.assignees.contains(&origin),
                    "task_assignee_required",
                )?;
                current.task.status = progress.status;
                current.task.logged_minutes = progress.logged_minutes;
                current.updated_at = now;
                entity = Some(*task_storage_id);
                DomainEventKind::TaskUpdated
            }
            ProviderCommand::RequestMilestoneCompletion {
                milestone_id,
                request,
                ..
            } => {
                coordinator(project, origin)?;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    milestone.status == Some(MilestoneStatus::InProgress) && !milestone.frozen,
                    "invalid_milestone_state",
                )?;
                let assigned: BTreeSet<_> = milestone
                    .assignments
                    .iter()
                    .map(|item| item.worker)
                    .collect();
                let rated: BTreeSet<_> = request
                    .worker_ratings
                    .iter()
                    .map(|item| item.worker)
                    .collect();
                require(
                    assigned == rated && rated.len() == request.worker_ratings.len(),
                    "worker_ratings_mismatch",
                )?;
                milestone.worker_ratings.clone_from(&request.worker_ratings);
                milestone.status = Some(MilestoneStatus::CompletionRequested);
                entity = Some(*milestone_id);
                DomainEventKind::MilestoneCompletionRequested
            }
            ProviderCommand::AcceptMilestoneCompletion {
                milestone_id,
                request,
                ..
            } => {
                client(project, origin)?;
                let coordinator = project.coordinator;
                let milestone = milestone_mut(project, *milestone_id)?;
                require(
                    milestone.status == Some(MilestoneStatus::CompletionRequested)
                        && !milestone.frozen,
                    "invalid_milestone_state",
                )?;
                let total = milestone.definition.total()?;
                self.credit(coordinator, milestone.definition.coordinator_fee)?;
                add_rating(
                    &mut self.worker_mut(coordinator)?.coordinator_score,
                    u16::from(request.coordinator_score.get()) * 100,
                    milestone.definition.coordinator_minutes,
                )?;
                for requirement in &milestone.definition.requirements {
                    let assignment = milestone
                        .assignments
                        .iter()
                        .find(|assignment| assignment.requirement_key == requirement.key)
                        .ok_or_else(Error::internal)?;
                    let rating = milestone
                        .worker_ratings
                        .iter()
                        .find(|rating| rating.worker == assignment.worker)
                        .ok_or_else(Error::internal)?;
                    let contribution = match request.team_rating {
                        TeamRating::Client(score) => self.score_policy.blend(rating.score, score),
                        TeamRating::DelegateToCoordinator => u16::from(rating.score.get()) * 100,
                    };
                    self.credit(assignment.worker, requirement.budget)?;
                    add_rating(
                        &mut self.worker_mut(assignment.worker)?.worker_score,
                        contribution,
                        requirement.minutes,
                    )?;
                }
                milestone.status = Some(MilestoneStatus::Completed);
                project.execution_escrow = project.execution_escrow.checked_sub(total)?;
                entity = Some(*milestone_id);
                DomainEventKind::MilestoneCompleted
            }
            _ => return Err(Error::bad("invalid_project_message")),
        };
        Ok(Effect::new(kind, entity, origin))
    }

    fn task_assignees(&self, task: &TaskDefinition) -> Result<()> {
        require(
            task.assignees
                .iter()
                .all(|account| self.workers.contains_key(account)),
            "unknown_task_assignee",
        )
    }
}

fn client(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(project.client == origin, "client_required")
}
fn coordinator(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(project.coordinator == origin, "coordinator_required")
}
fn participant(project: &ProjectView, origin: AccountId32) -> Result<()> {
    require(
        project.client == origin || project.coordinator == origin,
        "project_party_required",
    )
}
fn planning_active(project: &ProjectView) -> Result<()> {
    require(
        matches!(
            project.planning.status,
            PlanningStatus::Accepted | PlanningStatus::Delivered | PlanningStatus::Completed
        ) && !project.planning.frozen,
        "planning_not_accepted",
    )
}
fn revision(current: u64, expected: u64) -> Result<()> {
    require(current == expected, "revision_conflict")
}
fn bump(value: &mut u64) -> Result<()> {
    *value = value
        .checked_add(1)
        .ok_or_else(|| Error::domain("revision_exhausted"))?;
    Ok(())
}
fn proposal_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut ProposalView> {
    project
        .proposals
        .iter_mut()
        .find(|proposal| proposal.proposal_id == id)
        .ok_or_else(|| Error::domain("proposal_not_found"))
}
fn milestone_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut MilestoneView> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .find(|milestone| milestone.milestone_id == id)
        .ok_or_else(|| Error::domain("milestone_not_found"))
}
fn storage_mut(project: &mut ProjectView, id: EntityId) -> Result<&mut TaskStorageView> {
    project
        .proposals
        .iter_mut()
        .flat_map(|proposal| &mut proposal.milestones)
        .map(|milestone| &mut milestone.task_storage)
        .find(|storage| storage.task_storage_id == id)
        .ok_or_else(|| Error::domain("task_storage_not_found"))
}
fn task_mut(project: &mut ProjectView, storage: EntityId, id: u32) -> Result<&mut TaskView> {
    storage_mut(project, storage)?
        .tasks
        .iter_mut()
        .find(|task| task.task_id == id)
        .ok_or_else(|| Error::domain("task_not_found"))
}
fn proposal_definition(proposal: &ProposalView) -> ProposalDefinition {
    ProposalDefinition {
        title: proposal.title.clone(),
        description: proposal.description.clone(),
        milestones: proposal
            .milestones
            .iter()
            .map(|milestone| milestone.definition.clone())
            .collect(),
    }
}
fn make_milestones(
    proposal: &ProposalDefinition,
    previous: &[MilestoneView],
) -> Result<Vec<MilestoneView>> {
    proposal
        .milestones
        .iter()
        .map(|definition| {
            if let Some(existing) = previous
                .iter()
                .find(|milestone| milestone.definition.key == definition.key)
            {
                let mut milestone = existing.clone();
                milestone.definition = definition.clone();
                Ok(milestone)
            } else {
                let milestone_id = random_id()?;
                Ok(MilestoneView {
                    milestone_id,
                    definition: definition.clone(),
                    status: None,
                    assignments: Vec::new(),
                    worker_ratings: Vec::new(),
                    task_storage: TaskStorageView {
                        task_storage_id: random_id()?,
                        milestone_id,
                        tasks: Vec::new(),
                    },
                    frozen: false,
                })
            }
        })
        .collect()
}
fn empty_score() -> ReputationView {
    ReputationView {
        weighted_score_sum: 0,
        rated_minutes: 0,
    }
}
fn validate_score(score: &ReputationView) -> Result<()> {
    require(
        score.weighted_score_sum <= u128::from(score.rated_minutes) * 1000,
        "invalid_score_state",
    )
}
fn add_rating(score: &mut ReputationView, hundredths: u16, minutes: Minutes) -> Result<()> {
    score.rated_minutes = score
        .rated_minutes
        .checked_add(u64::from(minutes.get()))
        .ok_or_else(|| Error::domain("score_overflow"))?;
    score.weighted_score_sum = score
        .weighted_score_sum
        .checked_add(u128::from(hundredths) * u128::from(minutes.get()))
        .ok_or_else(|| Error::domain("score_overflow"))?;
    validate_score(score)
}
fn score_fraction(score: &ReputationView) -> (u128, u128) {
    if score.rated_minutes == 0 {
        (500, 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_scores_do_not_round_or_overflow() -> Result<()> {
        let low = ReputationView {
            weighted_score_sum: u128::from(u64::MAX) * 999 - 1,
            rated_minutes: u64::MAX,
        };
        let high = ReputationView {
            weighted_score_sum: u128::from(u64::MAX) * 999,
            rated_minutes: u64::MAX,
        };
        assert_eq!(compare_scores(&low, &high), Ordering::Less);
        assert_eq!(compare_scores(&high, &low), Ordering::Greater);
        assert_eq!(
            compare_scores(
                &empty_score(),
                &ReputationView {
                    weighted_score_sum: 3500,
                    rated_minutes: 7
                }
            ),
            Ordering::Equal
        );
        let mut score = empty_score();
        add_rating(&mut score, 1000, Minutes::new(100))?;
        add_rating(&mut score, 100, Minutes::new(1000))?;
        assert_eq!(score.weighted_score_sum, 200_000);
        assert_eq!(score.rated_minutes, 1100);
        Ok(())
    }

    #[test]
    fn selection_uses_only_the_reputation_for_the_requested_mode() -> Result<()> {
        let mut state = State::new(AccountId32::from_bytes([1; 32]))?;
        let first = AccountId32::from_bytes([2; 32]);
        let second = AccountId32::from_bytes([3; 32]);
        for account in [first, second] {
            state.apply(
                account,
                &ProviderCommand::RegisterWorker(generated_contracts::RegisterWorkerRequest {
                    display_name: "Candidate".into(),
                    qualifications: Qualifications {
                        role_ids: Vec::new(),
                        skill_ids: Vec::new(),
                    },
                    calendar: generated_contracts::CalendarDefinition {
                        default_weekly_minutes: Minutes::new(60),
                        overrides: Vec::new(),
                    },
                }),
                UnixSeconds::new(100),
            )?;
        }
        state.worker_mut(first)?.worker_score = ReputationView {
            weighted_score_sum: 900,
            rated_minutes: 1,
        };
        state.worker_mut(first)?.coordinator_score = ReputationView {
            weighted_score_sum: 100,
            rated_minutes: 1,
        };
        state.worker_mut(second)?.worker_score = ReputationView {
            weighted_score_sum: 200,
            rated_minutes: 1,
        };
        state.worker_mut(second)?.coordinator_score = ReputationView {
            weighted_score_sum: 1000,
            rated_minutes: 1,
        };
        assert_eq!(state.select(&[first, second], WorkerMode::Worker)?, first);
        assert_eq!(
            state.select(&[first, second], WorkerMode::Coordinator)?,
            second
        );
        Ok(())
    }

    #[cfg(feature = "storage-sqlite")]
    #[test]
    fn persisted_state_cannot_bypass_catalog_or_supply_invariants() -> Result<()> {
        let root = AccountId32::from_bytes([1; 32]);
        let mut state = State::new(root)?;
        state.roles.get_mut(&1).ok_or_else(Error::internal)?.name = "replacement".into();
        assert!(State::restore(&state.encode()?, root).is_err());
        let mut state = State::new(root)?;
        state.balances.insert(root, Money::new(1));
        assert!(State::restore(&state.encode()?, root).is_err());
        Ok(())
    }
}
