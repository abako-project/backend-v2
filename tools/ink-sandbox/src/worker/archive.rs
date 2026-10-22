//! Read-only codec for the pinned storage-v5 layout. It copies retained facts;
//! commands, permissions, prices and score updates still execute only in WASM.
use super::{directory::State, *};
use generated_contracts::*;
use parity_scale_codec::DecodeAll;
use provider_execution::migration::{
    ArchiveV1, BrampArchive, CheckpointV1, ReasonRecord, SkillMetadata,
};
use std::collections::BTreeMap;
const PAGE: usize = 8192;
type Reason = (
    EntityId,
    Option<EntityId>,
    AccountId32,
    String,
    UnixSeconds,
    DomainEventKind,
);
#[derive(Decode)]
struct Bank {
    enabled: bool,
    _mock_funding: bool,
    accounts: Vec<AccountId32>,
    minted: Money,
    deposits: Vec<EntityId>,
    withdrawals: Vec<EntityId>,
}
// PagedMapping handles encode zero bytes. Field order follows DirectoryData and
// Bank at manifest.sourceCommit; bundle identity rejects older persisted layouts.
#[derive(Decode)]
struct Directory {
    root: AccountId32,
    _calendar_code: H256,
    _reservation_controller: AccountId32,
    _reference_controller: AccountId32,
    roles: Vec<u32>,
    skills: Vec<u32>,
    workers: Vec<AccountId32>,
    requests: Vec<EntityId>,
    _proposals: Vec<EntityId>,
    score_policy: ScorePolicy,
    compensation_policy: CompensationPolicy,
    _project_code: Option<H256>,
    _references: Option<(H256, Account)>,
    projects: Vec<EntityId>,
    next_task_id: u32,
    bank: Bank,
    disputes: Vec<EntityId>,
    phase: u8,
    reasons: u64,
}
#[derive(Decode)]
struct WorkerRecord {
    qualifications: Qualifications,
    mode: WorkerMode,
    coordinator_eligible: bool,
    calendar: Account,
    calendar_id: EntityId,
    worker_score: ReputationView,
    coordinator_score: ReputationView,
}
struct Storage<'a> {
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
    fn cell<T: Decode>(&mut self, address: &Account, key: impl Encode) -> Result<T, Error> {
        let bytes = self
            .ext
            .execute_with(|| Contracts::get_storage(address.clone(), key.encode()))
            .map_err(|_| Error::InvalidState)?
            .ok_or(Error::InvalidState)?;
        if bytes.len() > 16 * 1024 {
            return Err(Error::InvalidState);
        }
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or(Error::Limit)?;
        T::decode_all(&mut &bytes[..]).map_err(|_| Error::InvalidState)
    }
    fn pages<T: Decode>(
        &mut self,
        length: u32,
        mut next: impl FnMut(&mut Self, u32) -> Result<Vec<u8>, Error>,
    ) -> Result<T, Error> {
        let length = length as usize;
        if length > self.remaining {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::with_capacity(length);
        for index in 0..length.div_ceil(PAGE) {
            let page = next(self, u32::try_from(index).map_err(|_| Error::Limit)?)?;
            if page.len() != PAGE.min(length - bytes.len()) {
                return Err(Error::InvalidState);
            }
            bytes.extend(page);
        }
        T::decode_all(&mut &bytes[..]).map_err(|_| Error::InvalidState)
    }
    fn root<T: Decode>(&mut self, address: &Account) -> Result<T, Error> {
        let length = self.cell(address, 0u32)?;
        self.pages(length, |reader, index| {
            reader.cell(address, (u32::MAX, index))
        })
    }
    fn child<T: Decode>(&mut self, address: &Account) -> Result<T, Error> {
        let (length, import): (u32, Option<(AccountId32, EntityId, u32)>) =
            self.cell(address, 0u32)?;
        if import.is_some() {
            return Err(Error::InvalidState);
        }
        self.pages(length, |reader, index| {
            reader.cell(address, (u32::MAX, index))
        })
    }
    fn value<T: Decode>(
        &mut self,
        address: &Account,
        namespace: u32,
        key: impl Encode + Copy,
    ) -> Result<T, Error> {
        let length = self.cell(address, (namespace, key, 0u32))?;
        self.pages(length, |reader, index| {
            reader.cell(address, (namespace, key, index + 1))
        })
    }
    fn workers(
        &mut self,
        state: &State,
        directory: &Directory,
        sealed: Option<bool>,
    ) -> Result<Vec<WorkerView>, Error> {
        let mut workers = Vec::new();
        for account in &directory.workers {
            let worker: WorkerRecord = self.value(&state.directory, 4, *account)?;
            let (calendar, parent, controller, live): (CalendarView, Account, Account, bool) =
                self.child(&worker.calendar)?;
            if calendar.owner != *account
                || calendar.calendar_id != worker.calendar_id
                || parent != state.directory
                || controller != state.directory
                || sealed.is_some_and(|expected| live != expected)
            {
                return Err(Error::InvalidState);
            }
            workers.push(WorkerView {
                account: *account,
                display_name: self.value(&state.directory, 17, *account)?,
                qualifications: worker.qualifications,
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
pub(super) fn workers(
    ext: &mut sp_io::TestExternalities,
    state: &State,
) -> Result<Vec<WorkerView>, Error> {
    let mut reader = Storage::new(ext);
    let directory: Directory = reader.root(&state.directory)?;
    reader.workers(state, &directory, None)
}
pub(super) fn read(
    ext: &mut sp_io::TestExternalities,
    state: &State,
    sealed: bool,
) -> Result<ArchiveV1, Error> {
    let mut reader = Storage::new(ext);
    let directory: Directory = reader.root(&state.directory)?;
    if directory.root != state.shell.info.root_account
        || directory.phase != if sealed { 2 } else { 1 }
        || !directory.bank.enabled
        || directory.reasons > MAX_SHELL as u64
    {
        return Err(Error::InvalidState);
    }
    let mut roles = BTreeMap::new();
    let mut skills = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    for id in &directory.roles {
        let entry: CatalogEntry = reader.value(&state.directory, 1, *id)?;
        if entry.id != *id {
            return Err(Error::InvalidState);
        }
        roles.insert(*id, entry);
    }
    for id in &directory.skills {
        let entry: CatalogEntry = reader.value(&state.directory, 2, *id)?;
        if entry.id != *id {
            return Err(Error::InvalidState);
        }
        skills.insert(*id, entry);
        metadata.insert(
            *id,
            SkillMetadata {
                category: reader.value(&state.directory, 18, *id)?,
                role_ids: reader.value(&state.directory, 3, *id)?,
            },
        );
    }
    let workers = reader
        .workers(state, &directory, Some(sealed))?
        .into_iter()
        .map(|w| (w.account, w))
        .collect();
    let mut requests = BTreeMap::new();
    for id in &directory.requests {
        let request: SkillRequestView = reader.value(&state.directory, 6, *id)?;
        if request.request_id != *id {
            return Err(Error::InvalidState);
        }
        requests.insert(*id, request);
    }
    let mut projects = BTreeMap::new();
    let mut disputes = BTreeMap::new();
    let mut local_reasons = BTreeMap::new();
    for id in &directory.projects {
        let address: Account = reader.value(&state.directory, 8, *id)?;
        let (parent, project, reasons, cases, live): (
            Account,
            ProjectView,
            Vec<Reason>,
            Vec<Dispute>,
            bool,
        ) = reader.child(&address)?;
        if parent != state.directory
            || project.project_id != *id
            || live != sealed
            || reasons.iter().any(|r| r.0 != *id)
        {
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
    if disputes.keys().copied().collect::<Vec<_>>() != directory.disputes {
        return Err(Error::InvalidState);
    }
    let mut balances = BTreeMap::new();
    let mut deposits = BTreeMap::new();
    let mut withdrawals = BTreeMap::new();
    for account in directory.bank.accounts {
        balances.insert(account, reader.value(&state.directory, 100, account)?);
    }
    for id in directory.bank.deposits {
        let deposit: DepositView = reader.value(&state.directory, 102, id)?;
        if deposit.deposit_id != id {
            return Err(Error::InvalidState);
        }
        deposits.insert(id, deposit);
    }
    for id in directory.bank.withdrawals {
        let withdrawal: WithdrawalView = reader.value(&state.directory, 103, id)?;
        if withdrawal.withdrawal_id != id {
            return Err(Error::InvalidState);
        }
        withdrawals.insert(id, withdrawal);
    }
    let mut reasons = Vec::new();
    for index in 0..directory.reasons {
        let bytes: Vec<u8> = reader.value(&state.directory, 19, index)?;
        let reason: Reason =
            Reason::decode_all(&mut &bytes[..]).map_err(|_| Error::InvalidState)?;
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
    let archive = ArchiveV1 {
        version: 1,
        checkpoint: CheckpointV1 {
            info: state.shell.info.clone(),
            roles,
            skills,
            skill_metadata: metadata,
            skill_requests: requests,
            score_policy: directory.score_policy,
            compensation_policy: directory.compensation_policy,
            workers,
            projects,
            balances,
            nonces: state.shell.nonces.clone(),
            receipts: state.shell.receipts.clone(),
            events: state.shell.events.clone(),
            reasons: reasons
                .into_iter()
                .map(|r| ReasonRecord {
                    project_id: r.0,
                    milestone_id: r.1,
                    origin: r.2,
                    reason: r.3,
                    occurred_at: r.4,
                    kind: r.5,
                })
                .collect(),
            disputes,
            bramp: BrampArchive {
                deposits,
                withdrawals,
            },
            next_task_id: directory.next_task_id,
            minted_units: directory.bank.minted,
            engine_version: 1,
        },
    };
    if serde_json::to_vec(&archive)
        .map_err(|_| Error::InvalidState)?
        .len()
        > MAX_SHELL
    {
        return Err(Error::Limit);
    }
    Ok(archive)
}
