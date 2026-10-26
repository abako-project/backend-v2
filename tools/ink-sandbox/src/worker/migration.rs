//! Complete private archive bridge. No historical business operation is replayed.
use super::{directory::*, *};
use generated_contracts::provider_core::{
    CatalogEntryCore, CatalogReferenceV1, ReasonCore, ReasonReferenceV1,
};
use generated_contracts::*;
use parity_scale_codec::DecodeAll;
use provider_execution::migration::CoreArchiveV5;
use std::collections::BTreeSet;

const CALENDAR_IMPORT_BATCH: usize = 64;

/// Fixed deployment settings; mutable catalogs and archive contents are not identity.
#[derive(Clone)]
pub struct WholeConfiguration {
    pub root: AccountId32,
    pub generation: ProviderInstanceId,
    pub mock_funding: bool,
    /// Signed execution budget; fresh migration uses the fixed whole-provider bound.
    pub gas_limit: Weight,
}
impl WholeConfiguration {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if !self.gas_limit.all_lte(crate::WHOLE_GAS)
            || self.root == AccountId32::from_bytes([0; 32])
        {
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
            authority: None,
        }
    }
}
#[derive(Encode)]
struct WorkerImport {
    account: AccountId32,
    content: generated_contracts::provider_core::MetadataReferenceV1,
    qualifications: QualificationCounts,
    mode: WorkerMode,
    coordinator_eligible: bool,
    calendar_id: EntityId,
    worker_score: ReputationView,
    coordinator_score: ReputationView,
}
#[derive(Clone, Copy, Encode)]
enum ImportTarget {
    Calendar(AccountId32),
    Project(EntityId),
}
#[derive(Clone, Copy, Encode)]
struct QualificationCounts {
    roles: u32,
    skills: u32,
}
#[derive(Encode)]
struct RequestImport {
    request_id: EntityId,
    requester: AccountId32,
    content: CatalogReferenceV1,
    roles: u32,
    status: SkillRequestStatus,
    skill_id: Option<u32>,
}
#[derive(Encode)]
struct ProjectImport {
    id: EntityId,
    proposals: u32,
    submissions: u32,
    cases: u32,
    evaluations: u32,
    escrow: Money,
}
#[derive(Encode)]
enum Record {
    Worker(WorkerImport),
    WorkerRole(AccountId32, u32, u32),
    WorkerSkill(AccountId32, u32, u32),
    Project(ProjectImport),
    ProjectProposal(EntityId, u32, EntityId, u32, QualificationCounts),
    ProjectMilestone(EntityId, u32, u32, EntityId, EntityId),
    ProjectSubmission(EntityId, u32, EntityId),
    ProposalRole(EntityId, u32, u32),
    ProposalSkill(EntityId, u32, u32),
    Case(EntityId, EntityId),
    Evaluation(EntityId, AccountId32),
    Deposit(DepositView),
    Withdrawal(WithdrawalView),
    Balance(AccountId32, Money),
    Issuance(Money),
    SkillRequest(RequestImport),
    RequestRole(EntityId, u32, u32),
    Policies(ScorePolicy, CompensationPolicy, u32),
    Reason(Reason),
    Catalog(CatalogKind, CatalogEntryCore, u32, CatalogReferenceV1),
    SkillRole(u32, u32, u32),
    CalendarDefinition(AccountId32, u32, u32, u32),
    CalendarOverrides(AccountId32, Vec<WeekOverride>),
    CalendarReservations(AccountId32, Vec<ReservationView>),
}
fn count(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::Limit)
}
type Reason = (
    EntityId,
    Option<EntityId>,
    AccountId32,
    ReasonReferenceV1,
    UnixSeconds,
    DomainEventKind,
);
fn reason_wire(reason: &ReasonCore) -> Reason {
    (
        reason.project_id,
        reason.milestone_id,
        reason.origin,
        reason.content.clone(),
        reason.occurred_at,
        reason.kind,
    )
}

fn write(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    gas: Weight,
    name: &str,
    args: impl Encode,
) -> Result<(), Error> {
    // Migration belongs to the whole-provider setup. The component
    // projection helper has a separate, smaller proof budget.
    let outcome = run(
        ext,
        bundles,
        Invocation {
            caller: Account::new(state.shell.info.root_account.into_bytes()),
            action: Action::Call {
                address: state.directory.clone(),
            },
            input: input(name, args),
            gas,
            timestamp_ms: 1_700_000_000_000,
        },
    )?;
    if outcome.status != Status::Success {
        return Err(Error::InvalidState);
    }
    Result::<Result<(), String>, u8>::decode_all(&mut &outcome.data[..])
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)
}
fn upload(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    gas: Weight,
    target: ImportTarget,
    bytes: &[u8],
) -> Result<(), Error> {
    for (index, page) in bytes.chunks(8192).enumerate() {
        write(
            ext,
            bundles,
            state,
            gas,
            "import_child_page",
            (
                target,
                u32::try_from(index).map_err(|_| Error::Limit)?,
                page,
            ),
        )?;
    }
    write(ext, bundles, state, gas, "finish_child_import", target)
}
fn upload_records(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    gas: Weight,
    records: &[Record],
) -> Result<(), Error> {
    let mut start = 0;
    while start < records.len() {
        let mut end = start;
        let mut bytes = 5usize;
        while end < records.len() && end - start < 64 {
            let next = bytes
                .checked_add(records[end].encoded_size())
                .ok_or(Error::Limit)?;
            if next > 8192 {
                break;
            }
            bytes = next;
            end += 1;
        }
        if end == start {
            return Err(Error::Limit);
        }
        write(
            ext,
            bundles,
            state,
            gas,
            "import_records",
            &records[start..end],
        )?;
        start = end;
    }
    Ok(())
}
fn upload_record(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    gas: Weight,
    record: &Record,
) -> Result<(), Error> {
    upload_records(ext, bundles, state, gas, std::slice::from_ref(record))
}

pub(super) fn initialize(
    bundles: &Bundles,
    archive: CoreArchiveV5,
    configuration: WholeConfiguration,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    configuration.validate()?;
    // Preserve the existing separation between import and signed execution:
    // low execution budgets may intentionally reject later calls after import.
    let gas = crate::WHOLE_GAS;
    let saved = &archive.checkpoint;
    if archive.version != 5
        || saved.engine_version != 5
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
            vec![CatalogEntryCore {
                id: 1,
                fixed: true,
                content: saved
                    .roles
                    .get(&1)
                    .ok_or(Error::InvalidState)?
                    .content
                    .clone(),
            }],
            Vec::new(),
            Vec::new(),
        ),
        reservation_controller: configuration.root,
        reference_controller: configuration.root,
        generation: Some(configuration.generation),
        gas_limit: GAS,
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
    write(&mut ext, bundles, &state, gas, "begin_import", ())?;
    let mut records = Vec::new();
    records.push(Record::Policies(
        saved.score_policy,
        saved.compensation_policy.clone(),
        saved.next_task_id,
    ));
    records.extend(saved.roles.values().cloned().map(|entry| {
        let origin = entry.content.clone();
        Record::Catalog(CatalogKind::Role, entry, 0, origin)
    }));
    for (id, entry) in &saved.skills {
        let metadata = saved.skill_metadata.get(id).ok_or(Error::InvalidState)?;
        records.push(Record::Catalog(
            CatalogKind::Skill,
            entry.clone(),
            count(metadata.role_ids.len())?,
            metadata.origin.clone(),
        ));
        for (index, role) in metadata.role_ids.iter().enumerate() {
            records.push(Record::SkillRole(*id, count(index)?, *role));
        }
    }
    for worker in saved.workers.values() {
        records.push(Record::Worker(WorkerImport {
            account: worker.account,
            content: worker.content.clone(),
            qualifications: QualificationCounts {
                roles: count(worker.qualifications.role_ids.len())?,
                skills: count(worker.qualifications.skill_ids.len())?,
            },
            mode: worker.mode,
            coordinator_eligible: worker.coordinator_eligible,
            calendar_id: worker.calendar.calendar_id,
            worker_score: worker.worker_score.clone(),
            coordinator_score: worker.coordinator_score.clone(),
        }));
        for (index, role) in worker.qualifications.role_ids.iter().enumerate() {
            records.push(Record::WorkerRole(worker.account, count(index)?, *role));
        }
        for (index, skill) in worker.qualifications.skill_ids.iter().enumerate() {
            records.push(Record::WorkerSkill(worker.account, count(index)?, *skill));
        }
    }
    for project in saved.projects.values() {
        let disputes: Vec<_> = saved
            .disputes
            .values()
            .filter(|d| d.project_id == project.project_id)
            .cloned()
            .collect();
        let escrow = project
            .planning
            .escrow
            .units()
            .checked_add(project.execution_escrow.units())
            .ok_or(Error::InvalidState)?;
        let submissions: Vec<_> = project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .flat_map(|m| &m.submissions)
            .map(|s| s.submission_id)
            .collect();
        records.push(Record::Project(ProjectImport {
            id: project.project_id,
            proposals: count(project.proposals.len())?,
            submissions: count(submissions.len())?,
            cases: count(disputes.len())?,
            evaluations: count(project.evaluations.len())?,
            escrow: Money::new(escrow),
        }));
        for (index, proposal) in project.proposals.iter().enumerate() {
            let mut roles = BTreeSet::new();
            let mut skills = BTreeSet::new();
            for requirement in proposal
                .milestones
                .iter()
                .flat_map(|m| &m.definition.requirements)
            {
                roles.insert(requirement.role_id);
                skills.extend(requirement.skill_ids.iter().copied());
            }
            records.push(Record::ProjectProposal(
                project.project_id,
                count(index)?,
                proposal.proposal_id,
                count(proposal.milestones.len())?,
                QualificationCounts {
                    roles: count(roles.len())?,
                    skills: count(skills.len())?,
                },
            ));
            for (i, role) in roles.into_iter().enumerate() {
                records.push(Record::ProposalRole(proposal.proposal_id, count(i)?, role));
            }
            for (i, skill) in skills.into_iter().enumerate() {
                records.push(Record::ProposalSkill(
                    proposal.proposal_id,
                    count(i)?,
                    skill,
                ));
            }
            for (i, milestone) in proposal.milestones.iter().enumerate() {
                records.push(Record::ProjectMilestone(
                    project.project_id,
                    count(index)?,
                    count(i)?,
                    milestone.milestone_id,
                    milestone.task_storage.task_storage_id,
                ));
            }
        }
        for (i, id) in submissions.into_iter().enumerate() {
            records.push(Record::ProjectSubmission(project.project_id, count(i)?, id));
        }
        for case in disputes {
            records.push(Record::Case(project.project_id, case.dispute_id));
        }
        for evaluation in &project.evaluations {
            records.push(Record::Evaluation(project.project_id, evaluation.author));
        }
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
    for request in saved.skill_requests.values() {
        records.push(Record::SkillRequest(RequestImport {
            request_id: request.request_id,
            requester: request.requester,
            content: request.content.clone(),
            roles: count(request.role_ids.len())?,
            status: request.status,
            skill_id: request.skill_id,
        }));
        for (i, role) in request.role_ids.iter().enumerate() {
            records.push(Record::RequestRole(request.request_id, count(i)?, *role));
        }
    }
    records.extend(
        saved
            .reasons
            .iter()
            .map(|reason| Record::Reason(reason_wire(reason))),
    );
    upload_records(&mut ext, bundles, &state, gas, &records)?;
    for worker in saved.workers.values() {
        upload_record(
            &mut ext,
            bundles,
            &state,
            gas,
            &Record::CalendarDefinition(
                worker.account,
                worker.calendar.definition.default_weekly_minutes.get(),
                u32::try_from(worker.calendar.definition.overrides.len())
                    .map_err(|_| Error::Limit)?,
                u32::try_from(worker.calendar.reservations.len()).map_err(|_| Error::Limit)?,
            ),
        )?;
        for items in worker
            .calendar
            .definition
            .overrides
            .chunks(CALENDAR_IMPORT_BATCH)
        {
            upload_record(
                &mut ext,
                bundles,
                &state,
                gas,
                &Record::CalendarOverrides(worker.account, items.to_vec()),
            )?;
        }
        for items in worker.calendar.reservations.chunks(CALENDAR_IMPORT_BATCH) {
            upload_record(
                &mut ext,
                bundles,
                &state,
                gas,
                &Record::CalendarReservations(worker.account, items.to_vec()),
            )?;
        }
        write(
            &mut ext,
            bundles,
            &state,
            gas,
            "finish_child_import",
            ImportTarget::Calendar(worker.account),
        )?;
    }
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
            .collect();
        let project = super::evidence_wire::Project::from_domain(
            project.clone(),
            &mut state.legacy_references,
        )?;
        let disputes = disputes
            .into_iter()
            .map(|case| {
                super::evidence_wire::Dispute::from_domain(
                    case.clone(),
                    &mut state.legacy_references,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let image = (&state.directory, &project, reasons, disputes, false).encode();
        upload(
            &mut ext,
            bundles,
            &state,
            gas,
            ImportTarget::Project(project.project_id),
            &image,
        )?;
    }
    // Compare all retained fields before making the fresh contract tree live.
    let imported = read_archive(&mut ext, bundles, &state, false)?;
    if serde_json::to_vec(&imported).map_err(|_| Error::InvalidState)?
        != serde_json::to_vec(&archive).map_err(|_| Error::InvalidState)?
    {
        return Err(Error::InvalidState);
    }
    write(&mut ext, bundles, &state, gas, "seal_import", ())?;
    read_archive(&mut ext, bundles, &state, true)?;
    Ok((ext, encode(&state)?))
}
pub(super) fn read_archive(
    ext: &mut sp_io::TestExternalities,
    _bundles: &Bundles,
    state: &State,
    sealed: bool,
) -> Result<CoreArchiveV5, Error> {
    super::archive::read(ext, state, sealed)
}
pub(super) async fn export(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
) -> Result<CoreArchiveV5, Error> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use generated_contracts::provider_core::SkillRequestCore;
    fn preserved(
        mut ext: sp_io::TestExternalities,
        failed: impl FnOnce(&mut sp_io::TestExternalities),
    ) -> sp_io::TestExternalities {
        let before = Checkpoint::capture(ext).unwrap();
        ext = before.clone().restore().unwrap();
        failed(&mut ext);
        let after = Checkpoint::capture(ext).unwrap();
        assert_eq!(before.to_bytes().unwrap(), after.to_bytes().unwrap());
        after.restore().unwrap()
    }
    fn typed_record_faults(
        mut ext: sp_io::TestExternalities,
        bundles: &Bundles,
        state: &State,
        gas: Weight,
        request: SkillRequestCore,
    ) -> sp_io::TestExternalities {
        let id = request.request_id;
        let record = Record::SkillRequest(RequestImport {
            request_id: id,
            requester: request.requester,
            content: request.content,
            roles: count(request.role_ids.len()).unwrap(),
            status: request.status,
            skill_id: request.skill_id,
        });
        upload_record(&mut ext, bundles, state, gas, &record).unwrap();
        // A valid Bank import provisions runtime funds and writes Directory
        // indexes before a later invalid child index rejects the entire batch.
        let balance = Record::Balance(AccountId32::from_bytes([7; 32]), Money::new(0));
        let before = Checkpoint::capture(ext).unwrap();
        let mut valid = before.clone().restore().unwrap();
        upload_record(&mut valid, bundles, state, gas, &balance).unwrap();
        assert_ne!(
            before.to_bytes().unwrap(),
            Checkpoint::capture(valid).unwrap().to_bytes().unwrap()
        );
        ext = before.restore().unwrap();
        ext = preserved(ext, |ext| {
            let records = vec![
                balance,
                Record::RequestRole(id, count(request.role_ids.len()).unwrap(), 1),
            ];
            assert!(write(ext, bundles, state, gas, "import_records", &records).is_err());
        });
        ext = preserved(ext, |ext| {
            assert!(write(ext, bundles, state, gas, "seal_import", ()).is_err());
        });
        for records in [
            vec![],
            vec![Record::RequestRole(id, 0, 1), Record::RequestRole(id, 0, 2)],
            (0..65).map(|i| Record::RequestRole(id, i, i + 1)).collect(),
        ] {
            ext = preserved(ext, |ext| {
                assert!(write(ext, bundles, state, gas, "import_records", &records).is_err());
            });
        }
        let records: Vec<_> = request
            .role_ids
            .into_iter()
            .enumerate()
            .map(|(i, role)| Record::RequestRole(id, count(i).unwrap(), role))
            .collect();
        upload_records(&mut ext, bundles, state, gas, &records).unwrap();
        ext
    }
    #[test]
    fn typed_directory_batches_and_child_finish_preserve_state_and_retry_then_seal() {
        let bundles = Bundles::pinned().unwrap();
        let gas = crate::WHOLE_GAS;
        let root = AccountId32::from_bytes([1; 32]);
        let owner = AccountId32::from_bytes([2; 32]);
        let fixture = DirectoryFixture {
            root,
            funded_accounts: vec![owner],
            catalog: (
                vec![CatalogEntryCore {
                    id: 1,
                    content: CatalogReferenceV1::Migration(PayloadHash::from_bytes([2; 32])),
                    fixed: true,
                }],
                Vec::new(),
                Vec::new(),
            ),
            reservation_controller: root,
            reference_controller: root,
            generation: Some(ProviderInstanceId::from_bytes([109; 16])),
            gas_limit: GAS,
            projects: true,
            money: Some(true),
        };
        let (mut ext, bytes) = super::super::directory::initialize(&bundles, fixture).unwrap();
        let state: State = serde_json::from_slice(&bytes).unwrap();
        write(&mut ext, &bundles, &state, gas, "begin_import", ()).unwrap();
        for id in 10..44 {
            upload_record(
                &mut ext,
                &bundles,
                &state,
                gas,
                &Record::Catalog(
                    CatalogKind::Skill,
                    CatalogEntryCore {
                        id,
                        content: CatalogReferenceV1::Migration(PayloadHash::from_bytes(
                            [u8::try_from(id).unwrap(); 32],
                        )),
                        fixed: false,
                    },
                    0,
                    CatalogReferenceV1::Migration(PayloadHash::from_bytes([5; 32])),
                ),
            )
            .unwrap();
        }
        let id = EntityId::from_bytes([51; 16]);
        // Descriptive labels no longer span pages. A historical request may
        // retain many distinct (subsequently deleted) role IDs instead.
        ext = typed_record_faults(
            ext,
            &bundles,
            &state,
            gas,
            SkillRequestCore {
                request_id: EntityId::from_bytes([52; 16]),
                requester: owner,
                content: CatalogReferenceV1::Migration(PayloadHash::from_bytes([77; 32])),
                role_ids: (1..2200).collect(),
                status: SkillRequestStatus::Rejected,
                skill_id: None,
            },
        );
        upload_record(
            &mut ext,
            &bundles,
            &state,
            gas,
            &Record::Worker(WorkerImport {
                account: owner,
                content: generated_contracts::provider_core::MetadataReferenceV1::Migration(
                    PayloadHash::from_bytes([71; 32]),
                ),
                qualifications: QualificationCounts {
                    roles: 0,
                    skills: 34,
                },
                mode: WorkerMode::Worker,
                coordinator_eligible: false,
                calendar_id: id,
                worker_score: ReputationView {
                    weighted_score_sum: 0,
                    rated_minutes: 0,
                },
                coordinator_score: ReputationView {
                    weighted_score_sum: 0,
                    rated_minutes: 0,
                },
            }),
        )
        .unwrap();
        // Declared qualifications cannot seal before every actual entry exists.
        ext = preserved(ext, |ext| {
            assert!(write(ext, &bundles, &state, gas, "seal_import", ()).is_err());
        });
        let skills: Vec<_> = (10..44)
            .enumerate()
            .map(|(i, skill)| Record::WorkerSkill(owner, count(i).unwrap(), skill))
            .collect();
        upload_records(&mut ext, &bundles, &state, gas, &skills).unwrap();
        let target = ImportTarget::Calendar(owner);
        ext = preserved(ext, |ext| {
            assert!(
                write(
                    ext,
                    &bundles,
                    &state,
                    gas,
                    "import_child_page",
                    (target, 1u32, vec![0u8])
                )
                .is_err()
            )
        });
        let mut overrides = Vec::new();
        for year in 2027..2047 {
            for week in 1..=50 {
                overrides.push(WeekOverride {
                    week: Week::new(year, week).unwrap(),
                    capacity: Minutes::new(600),
                });
            }
        }
        let calendar = CalendarView {
            calendar_id: id,
            owner,
            definition: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides,
            },
            reservations: Vec::new(),
        };
        upload_record(
            &mut ext,
            &bundles,
            &state,
            gas,
            &Record::CalendarDefinition(
                owner,
                calendar.definition.default_weekly_minutes.get(),
                u32::try_from(calendar.definition.overrides.len()).unwrap(),
                0,
            ),
        )
        .unwrap();
        ext = preserved(ext, |ext| {
            assert!(write(ext, &bundles, &state, gas, "finish_child_import", target).is_err());
        });
        // The first record writes successfully, but the duplicate second record
        // must revert the entire child call and the enclosing Directory staging.
        ext = preserved(ext, |ext| {
            let item = calendar.definition.overrides[0].clone();
            assert!(
                upload_record(
                    ext,
                    &bundles,
                    &state,
                    gas,
                    &Record::CalendarOverrides(owner, vec![item.clone(), item])
                )
                .is_err()
            );
        });
        for records in [
            Vec::new(),
            vec![calendar.definition.overrides[0].clone(); 65],
        ] {
            ext = preserved(ext, |ext| {
                assert!(
                    upload_record(
                        ext,
                        &bundles,
                        &state,
                        gas,
                        &Record::CalendarOverrides(owner, records)
                    )
                    .is_err()
                );
            });
        }
        for items in calendar.definition.overrides.chunks(CALENDAR_IMPORT_BATCH) {
            upload_record(
                &mut ext,
                &bundles,
                &state,
                gas,
                &Record::CalendarOverrides(owner, items.to_vec()),
            )
            .unwrap();
        }
        ext = preserved(ext, |ext| {
            assert!(
                upload_record(
                    ext,
                    &bundles,
                    &state,
                    gas,
                    &Record::CalendarOverrides(
                        owner,
                        vec![calendar.definition.overrides[0].clone()]
                    )
                )
                .is_err()
            );
        });
        write(
            &mut ext,
            &bundles,
            &state,
            gas,
            "finish_child_import",
            target,
        )
        .unwrap();
        // The first Calendar can seal, but a later unfinished child must revert
        // that earlier seal together with the entire Directory transaction.
        let second_owner = AccountId32::from_bytes([3; 32]);
        upload_record(
            &mut ext,
            &bundles,
            &state,
            gas,
            &Record::Worker(WorkerImport {
                account: second_owner,
                content: generated_contracts::provider_core::MetadataReferenceV1::Migration(
                    PayloadHash::from_bytes([72; 32]),
                ),
                qualifications: QualificationCounts {
                    roles: 0,
                    skills: 0,
                },
                mode: WorkerMode::Worker,
                coordinator_eligible: false,
                calendar_id: EntityId::from_bytes([53; 16]),
                worker_score: ReputationView {
                    weighted_score_sum: 0,
                    rated_minutes: 0,
                },
                coordinator_score: ReputationView {
                    weighted_score_sum: 0,
                    rated_minutes: 0,
                },
            }),
        )
        .unwrap();
        ext = preserved(ext, |ext| {
            assert!(write(ext, &bundles, &state, gas, "seal_import", ()).is_err());
        });
        upload_record(
            &mut ext,
            &bundles,
            &state,
            gas,
            &Record::CalendarDefinition(second_owner, 600, 0, 0),
        )
        .unwrap();
        write(
            &mut ext,
            &bundles,
            &state,
            gas,
            "finish_child_import",
            ImportTarget::Calendar(second_owner),
        )
        .unwrap();
        let exported = read_archive(&mut ext, &bundles, &state, false).unwrap();
        assert_eq!(exported.checkpoint.workers[&owner].calendar, calendar);
        // The component proof budget cannot traverse this larger retained
        // request. Failure remains atomic; the existing whole budget seals it.
        ext = preserved(ext, |ext| {
            assert!(write(ext, &bundles, &state, GAS, "seal_import", ()).is_err());
        });
        write(&mut ext, &bundles, &state, gas, "seal_import", ()).unwrap();
        let _exported = read_archive(&mut ext, &bundles, &state, true).unwrap();
        ext = preserved(ext, |ext| {
            assert!(
                write(
                    ext,
                    &bundles,
                    &state,
                    gas,
                    "import_child_page",
                    (target, 0u32, vec![0u8])
                )
                .is_err()
            )
        });
        assert!(read_archive(&mut ext, &bundles, &state, true).is_ok());
    }
}
