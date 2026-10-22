//! Complete private archive bridge. No historical business operation is replayed.
use super::{directory::*, *};
use generated_contracts::*;
use provider_execution::migration::{ArchiveV1, ReasonRecord};
use std::collections::BTreeSet;

/// Fixed deployment settings; mutable catalogs and archive contents are not identity.
#[derive(Clone)]
pub struct WholeConfiguration {
    pub root: AccountId32,
    pub generation: ProviderInstanceId,
    pub mock_funding: bool,
    pub gas_limit: Weight,
}
impl WholeConfiguration {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if !self.gas_limit.all_lte(GAS) || self.root == AccountId32::from_bytes([0; 32]) {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    pub(super) fn mode(&self) -> Mode {
        let configuration = H256::from(sp_core::hashing::blake2_256(
            &(
                b"whole-provider-v1",
                self.root,
                self.generation,
                self.mock_funding,
                self.gas_limit.ref_time(),
                self.gas_limit.proof_size(),
            )
                .encode(),
        ));
        Mode::Directory {
            root: self.root,
            gas: self.gas_limit,
            configuration,
            projects: true,
            money: true,
            whole: true,
        }
    }
}
#[derive(Encode)]
struct ProjectImport {
    id: EntityId,
    state: Vec<u8>,
    layout: Vec<(EntityId, Vec<(EntityId, EntityId)>)>,
    proposal_references: Vec<(EntityId, Qualifications)>,
    submissions: Vec<EntityId>,
    cases: Vec<EntityId>,
    escrow: Money,
    evaluations: Vec<AccountId32>,
}
#[derive(Encode)]
enum Record {
    Worker(WorkerView),
    Project(ProjectImport),
    Deposit(DepositView),
    Withdrawal(WithdrawalView),
    Balance(AccountId32, Money),
    Issuance(Money),
    SkillRequest(SkillRequestView),
    Category(u32, String),
    Policies(ScorePolicy, CompensationPolicy, u32),
    Reason(Vec<u8>),
}
type Reason = (
    EntityId,
    Option<EntityId>,
    AccountId32,
    String,
    UnixSeconds,
    DomainEventKind,
);
fn reason_wire(reason: &ReasonRecord) -> Reason {
    (
        reason.project_id,
        reason.milestone_id,
        reason.origin,
        reason.reason.clone(),
        reason.occurred_at,
        reason.kind,
    )
}

fn write(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    name: &str,
    args: impl Encode,
) -> Result<(), Error> {
    let result: Result<(), String> = read(
        ext,
        bundles,
        &state.directory,
        state.shell.info.root_account,
        name,
        args,
    )?;
    result.map_err(|_| Error::InvalidState)
}
pub(super) fn initialize(
    bundles: &Bundles,
    archive: ArchiveV1,
    configuration: WholeConfiguration,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    configuration.validate()?;
    let saved = &archive.checkpoint;
    if archive.version != 1
        || saved.engine_version != 1
        || saved.info.root_account != configuration.root
        || saved.info.provider_instance_id != configuration.generation
    {
        return Err(Error::InvalidState);
    }
    saved.shell().validate(configuration.root)?;
    if serde_json::to_vec(&archive)
        .map_err(|_| Error::InvalidState)?
        .len()
        > MAX_SHELL
    {
        return Err(Error::Limit);
    }
    let fixture = DirectoryFixture {
        root: configuration.root,
        funded_accounts: Vec::new(),
        catalog: (
            saved.roles.values().cloned().collect(),
            saved.skills.values().cloned().collect(),
            saved
                .skill_metadata
                .iter()
                .map(|(id, m)| SkillRoleAssociation {
                    skill_id: *id,
                    role_ids: m.role_ids.clone(),
                })
                .collect(),
        ),
        reservation_controller: configuration.root,
        reference_controller: configuration.root,
        generation: Some(configuration.generation),
        gas_limit: configuration.gas_limit,
        projects: true,
        money: Some(configuration.mock_funding),
    };
    let (mut ext, shell) = super::directory::initialize(bundles, fixture)?;
    let mut state: State = serde_json::from_slice(&shell).map_err(|_| Error::InvalidState)?;
    let Mode::Directory { configuration, .. } = configuration.mode() else {
        return Err(Error::InvalidState);
    };
    state.configuration = configuration;
    state.shell = saved.shell();
    write(&mut ext, bundles, &state, "begin_import", ())?;
    let mut records = Vec::new();
    records.push(Record::Policies(
        saved.score_policy,
        saved.compensation_policy.clone(),
        saved.next_task_id,
    ));
    records.extend(
        saved
            .skill_metadata
            .iter()
            .map(|(id, m)| Record::Category(*id, m.category.clone())),
    );
    records.extend(saved.workers.values().cloned().map(Record::Worker));
    for project in saved.projects.values() {
        let reasons: Vec<_> = saved
            .reasons
            .iter()
            .filter(|r| r.project_id == project.project_id)
            .map(reason_wire)
            .collect();
        let disputes: Vec<_> = saved
            .disputes
            .values()
            .filter(|d| d.project_id == project.project_id)
            .cloned()
            .collect();
        let proposal_references = project
            .proposals
            .iter()
            .map(|p| {
                let mut roles = BTreeSet::new();
                let mut skills = BTreeSet::new();
                for requirement in p.milestones.iter().flat_map(|m| &m.definition.requirements) {
                    roles.insert(requirement.role_id);
                    skills.extend(requirement.skill_ids.iter().copied());
                }
                (
                    p.proposal_id,
                    Qualifications {
                        role_ids: roles.into_iter().collect(),
                        skill_ids: skills.into_iter().collect(),
                    },
                )
            })
            .collect();
        let escrow = project
            .planning
            .escrow
            .units()
            .checked_add(project.execution_escrow.units())
            .ok_or(Error::InvalidState)?;
        records.push(Record::Project(ProjectImport {
            id: project.project_id,
            state: (project, &reasons, &disputes).encode(),
            layout: project
                .proposals
                .iter()
                .map(|p| {
                    (
                        p.proposal_id,
                        p.milestones
                            .iter()
                            .map(|m| (m.milestone_id, m.task_storage.task_storage_id))
                            .collect(),
                    )
                })
                .collect(),
            proposal_references,
            submissions: project
                .proposals
                .iter()
                .flat_map(|p| &p.milestones)
                .flat_map(|m| &m.submissions)
                .map(|s| s.submission_id)
                .collect(),
            cases: disputes.iter().map(|d| d.dispute_id).collect(),
            escrow: Money::new(escrow),
            evaluations: project.evaluations.iter().map(|e| e.author).collect(),
        }));
    }
    records.extend(
        saved
            .balances
            .iter()
            .map(|(account, amount)| Record::Balance(*account, *amount)),
    );
    records.extend(saved.bramp.deposits.values().cloned().map(Record::Deposit));
    records.extend(
        saved
            .bramp
            .withdrawals
            .values()
            .cloned()
            .map(Record::Withdrawal),
    );
    records.push(Record::Issuance(saved.minted_units));
    records.extend(
        saved
            .skill_requests
            .values()
            .cloned()
            .map(Record::SkillRequest),
    );
    records.extend(
        saved
            .reasons
            .iter()
            .map(|r| Record::Reason(reason_wire(r).encode())),
    );
    for record in records {
        if record.encoded_size() + 4 > MAX_INPUT {
            return Err(Error::Limit);
        }
        write(&mut ext, bundles, &state, "import_record", record)?;
    }
    // Compare all retained fields before making the fresh contract tree live.
    let imported = read_archive(&mut ext, bundles, &state, false)?;
    if serde_json::to_vec(&imported).map_err(|_| Error::InvalidState)?
        != serde_json::to_vec(&archive).map_err(|_| Error::InvalidState)?
    {
        return Err(Error::InvalidState);
    }
    write(&mut ext, bundles, &state, "seal_import", ())?;
    read_archive(&mut ext, bundles, &state, true)?;
    Ok((ext, encode(&state)?))
}
pub(super) fn read_archive(
    ext: &mut sp_io::TestExternalities,
    _bundles: &Bundles,
    state: &State,
    sealed: bool,
) -> Result<ArchiveV1, Error> {
    super::archive::read(ext, state, sealed)
}
pub(super) async fn export(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
) -> Result<ArchiveV1, Error> {
    if !matches!(mode, Mode::Directory { whole: true, .. }) {
        return Err(Error::IncompleteCoverage);
    }
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    let mut ext = stored.checkpoint.restore()?;
    let archive = read_archive(&mut ext, bundles, &state, true)?;
    tx.rollback().await?;
    Ok(archive)
}
