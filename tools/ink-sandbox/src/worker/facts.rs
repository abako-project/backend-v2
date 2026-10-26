//! Read-only codec for pinned ordinary ink storage. This owner codec copies facts;
//! commands, permissions, prices and score updates still execute only in WASM.
use super::{directory::State, *};
use generated_contracts::provider_core::{
    CatalogEntryCore, ReasonCore, ReasonReferenceV1, SkillMetadataCore, SkillRequestCore,
    WorkerCore,
};
use generated_contracts::*;
use parity_scale_codec::DecodeAll;
use std::collections::{BTreeMap, BTreeSet};
/// Current facts read from ink cells. Transport history lives only in the shell.
pub(super) struct Facts {
    pub roles: BTreeMap<u32, CatalogEntryCore>,
    pub skills: BTreeMap<u32, CatalogEntryCore>,
    pub skill_metadata: BTreeMap<u32, SkillMetadataCore>,
    pub skill_requests: BTreeMap<EntityId, SkillRequestCore>,
    pub score_policy: ScorePolicy,
    pub compensation_policy: CompensationPolicy,
    pub workers: BTreeMap<AccountId32, WorkerCore>,
    pub projects: BTreeMap<EntityId, generated_contracts::provider_core::ProjectCore>,
    pub balances: BTreeMap<AccountId32, Money>,
    pub reasons: Vec<ReasonCore>,
    pub disputes: BTreeMap<EntityId, Dispute>,
    pub next_task_id: u32,
}

pub(super) fn next_task_id(
    ext: &mut sp_io::TestExternalities,
    state: &State,
) -> Result<u32, Error> {
    let header: Directory = Storage::new(ext).cell(&state.directory, 0u32)?;
    Ok(header.next_task_id)
}
pub(super) fn deposit(
    ext: &mut sp_io::TestExternalities,
    state: &State,
    id: EntityId,
) -> Result<Option<DepositView>, Error> {
    Storage::new(ext).optional_cell(&state.directory, (102u32, id))
}
pub(super) fn withdrawal(
    ext: &mut sp_io::TestExternalities,
    state: &State,
    id: EntityId,
) -> Result<Option<WithdrawalView>, Error> {
    Storage::new(ext).optional_cell(&state.directory, (103u32, id))
}

pub(super) type Reason = (
    EntityId,
    Option<EntityId>,
    AccountId32,
    ReasonReferenceV1,
    UnixSeconds,
    DomainEventKind,
);
#[derive(Decode)]
struct Bank {
    enabled: bool,
    _mock_funding: bool,
    #[codec(skip)]
    accounts: Vec<AccountId32>,
    _minted: Money,
    #[codec(skip)]
    deposits: Vec<EntityId>,
    #[codec(skip)]
    withdrawals: Vec<EntityId>,
}
// Standard ink Mapping/StorageVec handles encode zero bytes. Field order follows DirectoryData and
// Bank at manifest.sourceCommit; bundle identity rejects older persisted layouts.
#[derive(Decode)]
struct Directory {
    root: AccountId32,
    _calendar_code: H256,
    _reservation_controller: AccountId32,
    _reference_controller: AccountId32,
    #[codec(skip)]
    roles: Vec<u32>,
    #[codec(skip)]
    skills: Vec<u32>,
    #[codec(skip)]
    workers: Vec<AccountId32>,
    #[codec(skip)]
    requests: Vec<EntityId>,
    #[codec(skip)]
    _proposals: Vec<EntityId>,
    score_policy: ScorePolicy,
    compensation_policy: CompensationPolicy,
    _project_code: Option<H256>,
    #[codec(skip)]
    projects: Vec<EntityId>,
    next_task_id: u32,
    bank: Bank,
    #[codec(skip)]
    disputes: Vec<EntityId>,
    reasons: u64,
}
#[derive(Decode)]
struct WorkerRecord {
    content: generated_contracts::provider_core::MetadataReferenceV1,
    qualifications: QualificationCounts,
    mode: WorkerMode,
    coordinator_eligible: bool,
    calendar: Account,
    calendar_id: EntityId,
    worker_score: ReputationView,
    coordinator_score: ReputationView,
}
#[derive(Decode)]
struct QualificationCounts {
    roles: u32,
    skills: u32,
}
#[derive(Decode)]
struct ProjectProposalRecord {
    id: EntityId,
    milestones: u32,
}
#[derive(Decode)]
struct RequestRecord {
    request_id: EntityId,
    requester: AccountId32,
    content: generated_contracts::provider_core::CatalogReferenceV1,
    roles: u32,
    status: SkillRequestStatus,
    skill_id: Option<u32>,
}
// Standard ink Lazy/Mapping cells in the pinned Calendar contract. Each decoded
// value is fixed-size; no root reconstruction or byte-page codec is involved.
#[derive(Decode)]
struct CalendarHeader {
    calendar_id: EntityId,
    owner: AccountId32,
    directory: Account,
    controller: Account,
    default_minutes: u32,
    overrides: u32,
    reservations: u32,
    reserved_weeks: u32,
}
pub(super) struct Storage<'a> {
    ext: &'a mut sp_io::TestExternalities,
    remaining: usize,
}
impl<'a> Storage<'a> {
    fn new(ext: &'a mut sp_io::TestExternalities) -> Self {
        Self {
            ext,
            remaining: MAX_SHELL,
        }
    }
    fn optional_cell<T: Decode>(
        &mut self,
        address: &Account,
        key: impl Encode,
    ) -> Result<Option<T>, Error> {
        let Some(bytes) = self
            .ext
            .execute_with(|| Contracts::get_storage(address.clone(), key.encode()))
            .map_err(|_| Error::InvalidState)?
        else {
            return Ok(None);
        };
        if bytes.len() > 16 * 1024 {
            return Err(Error::InvalidState);
        }
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or(Error::Limit)?;
        T::decode_all(&mut &bytes[..])
            .map(Some)
            .map_err(|_| Error::InvalidState)
    }
    pub(super) fn cell<T: Decode>(
        &mut self,
        address: &Account,
        key: impl Encode,
    ) -> Result<T, Error> {
        self.optional_cell(address, key)?.ok_or(Error::InvalidState)
    }
    fn index<T: Decode + Ord>(&mut self, address: &Account, key: u32) -> Result<Vec<T>, Error> {
        let count: u32 = self.optional_cell(address, key)?.unwrap_or(0);
        if count as usize > self.remaining {
            return Err(Error::Limit);
        }
        let mut values = Vec::new();
        for i in 0..count {
            let value = self.cell(address, (key, i))?;
            if values.last().is_some_and(|previous| previous >= &value) {
                return Err(Error::InvalidState);
            }
            values.push(value);
        }
        Ok(values)
    }
    pub(super) fn list<T: Decode>(
        &mut self,
        address: &Account,
        namespace: u32,
        owner: impl Encode + Copy,
        count: u32,
    ) -> Result<Vec<T>, Error> {
        if count as usize > self.remaining {
            return Err(Error::Limit);
        }
        (0..count)
            .map(|i| self.cell(address, (namespace, (owner, i))))
            .collect()
    }
    fn root(&mut self, state: &directory::State) -> Result<Directory, Error> {
        let address = &state.directory;
        let mut header: Directory = self.cell(address, 0u32)?;
        header.roles = self.index(address, 50)?;
        header.skills = self.index(address, 51)?;
        header.workers = self.index(address, 52)?;
        header.requests = self.index(address, 53)?;
        header._proposals = self.index(address, 54)?;
        header.projects = self.index(address, 55)?;
        header.disputes = self.index(address, 56)?;
        header.bank.accounts = self.index(address, 150)?;
        header.bank.deposits = self.index(address, 151)?;
        header.bank.withdrawals = self.index(address, 152)?;
        Ok(header)
    }
    pub(super) fn value<T: Decode>(
        &mut self,
        address: &Account,
        namespace: u32,
        key: impl Encode + Copy,
    ) -> Result<T, Error> {
        self.cell(address, (namespace, key))
    }
    fn calendar(&mut self, address: &Account) -> Result<(CalendarView, Account, Account), Error> {
        let header: CalendarHeader = self.cell(address, 0x4341_4c01u32)?;
        if header.reserved_weeks > header.reservations {
            return Err(Error::InvalidState);
        }
        let records = u64::from(header.overrides) + u64::from(header.reservations);
        if records > u64::try_from(self.remaining).map_err(|_| Error::Limit)? {
            return Err(Error::Limit);
        }
        let mut overrides = Vec::new();
        for index in 0..header.overrides {
            let week: Week = self.cell(address, (0x4341_4c03u32, index))?;
            let capacity = self.cell(address, (0x4341_4c02u32, week))?;
            overrides.push(WeekOverride { week, capacity });
        }
        let mut reservations = Vec::new();
        let mut totals = BTreeMap::<Week, u32>::new();
        for index in 0..header.reservations {
            let item: ReservationView = self.cell(address, (0x4341_4c04u32, index))?;
            let total = totals.entry(item.week).or_default();
            *total = total
                .checked_add(item.minutes.get())
                .ok_or(Error::InvalidState)?;
            reservations.push(item);
        }
        if totals.len() != usize::try_from(header.reserved_weeks).map_err(|_| Error::Limit)? {
            return Err(Error::InvalidState);
        }
        for index in 0..header.reserved_weeks {
            let week: Week = self.cell(address, (0x4341_4c06u32, index))?;
            let total: u32 = self.cell(address, (0x4341_4c05u32, week))?;
            if totals.remove(&week) != Some(total) {
                return Err(Error::InvalidState);
            }
        }
        Ok((
            CalendarView {
                calendar_id: header.calendar_id,
                owner: header.owner,
                definition: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(header.default_minutes),
                    overrides,
                },
                reservations,
            },
            header.directory,
            header.controller,
        ))
    }
    fn workers(&mut self, state: &State, directory: &Directory) -> Result<Vec<WorkerCore>, Error> {
        let mut workers = Vec::new();
        for account in &directory.workers {
            let worker: WorkerRecord = self.value(&state.directory, 4, *account)?;
            let (calendar, parent, controller): (CalendarView, Account, Account) =
                self.calendar(&worker.calendar)?;
            if calendar.owner != *account
                || calendar.calendar_id != worker.calendar_id
                || parent != state.directory
                || controller != state.directory
            {
                return Err(Error::InvalidState);
            }
            workers.push(WorkerCore {
                account: *account,
                content: worker.content,
                qualifications: Qualifications {
                    role_ids: self.list(
                        &state.directory,
                        21,
                        *account,
                        worker.qualifications.roles,
                    )?,
                    skill_ids: self.list(
                        &state.directory,
                        22,
                        *account,
                        worker.qualifications.skills,
                    )?,
                },
                mode: worker.mode,
                coordinator_eligible: worker.coordinator_eligible,
                calendar,
                worker_score: worker.worker_score,
                coordinator_score: worker.coordinator_score,
            });
        }
        Ok(workers)
    }
}
pub(super) fn read(ext: &mut sp_io::TestExternalities, state: &State) -> Result<Facts, Error> {
    let mut reader = Storage::new(ext);
    let directory: Directory = reader.root(state)?;
    if directory.root != state.shell.info.root_account
        || !directory.bank.enabled
        || directory.reasons > MAX_SHELL as u64
    {
        return Err(Error::InvalidState);
    }
    let mut roles = BTreeMap::new();
    let mut skills = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    for id in &directory.roles {
        let entry: CatalogEntryCore = reader.value(&state.directory, 1, *id)?;
        if entry.id != *id {
            return Err(Error::InvalidState);
        }
        roles.insert(*id, entry);
    }
    for id in &directory.skills {
        let entry: CatalogEntryCore = reader.value(&state.directory, 2, *id)?;
        if entry.id != *id {
            return Err(Error::InvalidState);
        }
        skills.insert(*id, entry);
        metadata.insert(
            *id,
            SkillMetadataCore {
                origin: reader.value(&state.directory, 18, *id)?,
                role_ids: {
                    let count = reader.value(&state.directory, 3, *id)?;
                    reader.list(&state.directory, 20, *id, count)?
                },
            },
        );
    }
    let workers = reader
        .workers(state, &directory)?
        .into_iter()
        .map(|w| (w.account, w))
        .collect();
    let mut requests = BTreeMap::new();
    for id in &directory.requests {
        let record: RequestRecord = reader.value(&state.directory, 6, *id)?;
        let request = SkillRequestCore {
            request_id: record.request_id,
            requester: record.requester,
            content: record.content,
            role_ids: reader.list(&state.directory, 23, *id, record.roles)?,
            status: record.status,
            skill_id: record.skill_id,
        };
        if request.request_id != *id {
            return Err(Error::InvalidState);
        }
        requests.insert(*id, request);
    }
    let mut projects = BTreeMap::new();
    let mut disputes = BTreeMap::new();
    let mut local_reasons = BTreeMap::new();
    let mut proposal_ids = BTreeSet::new();
    for id in &directory.projects {
        let address: Account = reader.value(&state.directory, 8, *id)?;
        let super::project_storage::StoredProject {
            directory: parent,
            project,
            reasons,
            cases,
        } = super::project_storage::read(&mut reader, &address)?;
        if parent != state.directory
            || project.project_id != *id
            || reasons.iter().any(|r| r.0 != *id)
        {
            return Err(Error::InvalidState);
        }
        // Directory's lookup/guard facts must describe the actual Project tree.
        // Comparing only the Project projection would miss an incomplete secondary index.
        let registered: EntityId = reader.cell(&state.directory, (9u32, &address))?;
        let count: u32 = reader
            .optional_cell(&state.directory, (10u32, *id))?
            .unwrap_or(0);
        if registered != *id || count as usize != project.proposals.len() {
            return Err(Error::InvalidState);
        }
        for (index, proposal) in project.proposals.iter().enumerate() {
            let index = u32::try_from(index).map_err(|_| Error::Limit)?;
            let record: ProjectProposalRecord = reader.value(&state.directory, 26, (*id, index))?;
            if record.id != proposal.proposal_id
                || record.milestones as usize != proposal.milestones.len()
                || !proposal_ids.insert(proposal.proposal_id)
            {
                return Err(Error::InvalidState);
            }
            let mut roles = BTreeSet::new();
            let mut skills = BTreeSet::new();
            for (i, milestone) in proposal.milestones.iter().enumerate() {
                let actual: (EntityId, EntityId) = reader.value(
                    &state.directory,
                    27,
                    (*id, index, u32::try_from(i).map_err(|_| Error::Limit)?),
                )?;
                if actual
                    != (
                        milestone.milestone_id,
                        milestone.task_storage.task_storage_id,
                    )
                {
                    return Err(Error::InvalidState);
                }
                for requirement in &milestone.definition.requirements {
                    roles.insert(requirement.role_id);
                    skills.extend(requirement.skill_ids.iter().copied());
                }
            }
            let counts: QualificationCounts =
                reader.value(&state.directory, 7, proposal.proposal_id)?;
            let actual_roles: Vec<u32> =
                reader.list(&state.directory, 24, proposal.proposal_id, counts.roles)?;
            let actual_skills: Vec<u32> =
                reader.list(&state.directory, 25, proposal.proposal_id, counts.skills)?;
            if actual_roles != roles.into_iter().collect::<Vec<_>>()
                || actual_skills != skills.into_iter().collect::<Vec<_>>()
            {
                return Err(Error::InvalidState);
            }
        }
        let count = reader
            .optional_cell(&state.directory, (11u32, *id))?
            .unwrap_or(0);
        let actual_submissions: Vec<EntityId> = reader.list(&state.directory, 28, *id, count)?;
        let expected_submissions: Vec<_> = project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.submissions)
            .map(|s| s.submission_id)
            .collect();
        if actual_submissions != expected_submissions {
            return Err(Error::InvalidState);
        }
        for case in cases {
            let owner: EntityId = reader.value(&state.directory, 12, case.dispute_id)?;
            if case.project_id != *id
                || owner != *id
                || disputes.insert(case.dispute_id, case).is_some()
            {
                return Err(Error::InvalidState);
            }
        }
        projects.insert(*id, project);
        local_reasons.insert(*id, reasons);
    }
    if proposal_ids.into_iter().collect::<Vec<_>>() != directory._proposals
        || disputes.keys().copied().collect::<Vec<_>>() != directory.disputes
    {
        return Err(Error::InvalidState);
    }
    let mut balances = BTreeMap::new();
    for account in directory.bank.accounts {
        balances.insert(account, reader.value(&state.directory, 100, account)?);
    }
    for id in directory.bank.deposits {
        let deposit: DepositView = reader.value(&state.directory, 102, id)?;
        if deposit.deposit_id != id {
            return Err(Error::InvalidState);
        }
    }
    for id in directory.bank.withdrawals {
        let withdrawal: WithdrawalView = reader.value(&state.directory, 103, id)?;
        if withdrawal.withdrawal_id != id {
            return Err(Error::InvalidState);
        }
    }
    let mut reasons = Vec::new();
    for index in 0..directory.reasons {
        let reason: Reason = reader.value(&state.directory, 19, index)?;
        reasons.push(reason);
    }
    for (id, local) in local_reasons {
        if reasons
            .iter()
            .filter(|r| r.0 == id)
            .cloned()
            .collect::<Vec<_>>()
            != local
        {
            return Err(Error::InvalidState);
        }
    }
    Ok(Facts {
        roles,
        skills,
        skill_metadata: metadata,
        skill_requests: requests,
        score_policy: directory.score_policy,
        compensation_policy: directory.compensation_policy,
        workers,
        projects,
        balances,
        disputes,
        next_task_id: directory.next_task_id,
        reasons: reasons
            .into_iter()
            .map(|r| ReasonCore {
                project_id: r.0,
                milestone_id: r.1,
                origin: r.2,
                content: r.3,
                occurred_at: r.4,
                kind: r.5,
            })
            .collect(),
    })
}
