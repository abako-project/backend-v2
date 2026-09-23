use std::{cmp::Ordering, collections::BTreeSet};

use generated_contracts::{
    AccountId32, AssignmentView, CalendarView, CatalogEntry, CatalogKind, DomainEventKind,
    MilestoneView, Minutes, Money, PlanningStatus, PlanningView, ProjectView, ProviderCommand,
    Qualifications, ReputationView, ReservationView, TaskDefinition, UnixSeconds, WorkerMode,
    WorkerView,
};
use rand::RngExt;

use super::{Effect, State, compare_scores, empty_score};
use crate::{Error, Result, calendar, random_id, require};

impl State {
    fn root(&self, origin: AccountId32) -> Result<()> {
        require(origin == self.info.root_account, "system_origin_required")
    }

    pub(super) fn qualifications(&self, qualifications: &Qualifications) -> Result<()> {
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

    pub(super) fn proposal_qualifications(
        &self,
        proposal: &generated_contracts::ProposalDefinition,
    ) -> Result<()> {
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

    pub(super) fn worker_mut(&mut self, account: AccountId32) -> Result<&mut WorkerView> {
        self.workers
            .get_mut(&account)
            .ok_or_else(|| Error::domain("worker_not_found"))
    }

    pub(super) fn credit(&mut self, account: AccountId32, amount: Money) -> Result<()> {
        let balance = self.balances.entry(account).or_default();
        *balance = balance.checked_add(amount)?;
        Ok(())
    }

    pub(super) fn debit(&mut self, account: AccountId32, amount: Money) -> Result<()> {
        let balance = self.balances.entry(account).or_default();
        require(*balance >= amount, "insufficient_balance")?;
        *balance = balance.checked_sub(amount)?;
        Ok(())
    }

    pub(super) fn apply(
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
                self.delete_catalog_entry(request.kind, request.id, origin)?
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
                self.create_project(origin, &request.title, &request.description, now)?
            }
            _ => self.apply_project_command(origin, command, now)?,
        };
        Ok(effect)
    }

    fn delete_catalog_entry(
        &mut self,
        kind: CatalogKind,
        id: u32,
        origin: AccountId32,
    ) -> Result<Effect> {
        self.root(origin)?;
        require(
            !(kind == CatalogKind::Role && id == 1),
            "fixed_coordinator_role",
        )?;
        let references_worker = self.workers.values().any(|worker| {
            if kind == CatalogKind::Role {
                worker.qualifications.role_ids.contains(&id)
            } else {
                worker.qualifications.skill_ids.contains(&id)
            }
        });
        let references_proposal = self
            .projects
            .values()
            .flat_map(|project| &project.proposals)
            .flat_map(|proposal| &proposal.milestones)
            .flat_map(|milestone| &milestone.definition.requirements)
            .any(|requirement| {
                if kind == CatalogKind::Role {
                    requirement.role_id == id
                } else {
                    requirement.skill_ids.contains(&id)
                }
            });
        require(
            !references_worker && !references_proposal,
            "catalog_entry_in_use",
        )?;
        let catalog = if kind == CatalogKind::Role {
            &mut self.roles
        } else {
            &mut self.skills
        };
        require(catalog.remove(&id).is_some(), "catalog_entry_not_found")?;
        if kind == CatalogKind::Skill {
            self.skill_metadata.remove(&id);
        }
        Ok(Effect::new(DomainEventKind::CatalogUpdated, None, origin))
    }

    fn create_project(
        &mut self,
        origin: AccountId32,
        title: &str,
        description: &str,
        now: UnixSeconds,
    ) -> Result<Effect> {
        // Initial selection has no quoted window yet: this indicative filter uses
        // the current week. Quote acceptance reserves against fresh capacity.
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
                title: title.into(),
                description: description.into(),
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
                active_dispute_id: None,
            },
        );
        let mut effect = Effect::new(DomainEventKind::ProjectCreated, Some(project_id), origin);
        effect.recipients.insert(coordinator);
        Ok(effect)
    }

    fn apply_project_command(
        &mut self,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        let project_id = command
            .project_id()
            .ok_or_else(|| Error::bad("invalid_target"))?;
        let mut project = self
            .projects
            .remove(&project_id)
            .ok_or_else(|| Error::domain("project_not_found"))?;
        require(!project.cancelled, "project_cancelled")?;
        if project.active_dispute_id.is_some() {
            match command {
                ProviderCommand::RespondDispute { .. } => {}
                ProviderCommand::OpenDispute(_) => {
                    return Err(super::disputes::DisputeError::AlreadyOpen.into());
                }
                _ => return Err(super::disputes::DisputeError::ProjectFrozen.into()),
            }
        }
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
        Ok(effect)
    }

    pub(super) fn select(
        &self,
        candidates: &[AccountId32],
        mode: WorkerMode,
    ) -> Result<AccountId32> {
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

    pub(super) fn assign(
        &mut self,
        project: &ProjectView,
        milestone: &mut MilestoneView,
    ) -> Result<()> {
        let window = milestone.definition.window;
        let reference = ReservationView {
            project_id: project.project_id,
            milestone_id: Some(milestone.milestone_id),
            requirement_key: None,
            week: window.start(),
            minutes: Minutes::ZERO,
        };
        calendar::reserve(
            &mut self.worker_mut(project.coordinator)?.calendar,
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
        milestone.status = Some(generated_contracts::MilestoneStatus::InProgress);
        Ok(())
    }

    pub(super) fn task_assignees(&self, task: &TaskDefinition) -> Result<()> {
        require(
            task.assignees
                .iter()
                .all(|account| self.workers.contains_key(account)),
            "unknown_task_assignee",
        )
    }
}
