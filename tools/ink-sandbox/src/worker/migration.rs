//! Complete private archive bridge. No historical business operation is replayed.
use super::{directory::*, *};
use generated_contracts::provider_core::{
    CatalogEntryCore, CatalogReferenceV1, ReasonCore, ReasonReferenceV1, SkillRequestCore,
};
use generated_contracts::*;
use provider_execution::migration::CoreArchiveV4;
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
    qualifications: Qualifications,
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
#[derive(Encode)]
struct ProjectImport {
    id: EntityId,
    layout: Vec<(EntityId, Vec<(EntityId, EntityId)>)>,
    proposal_references: Vec<(EntityId, Qualifications)>,
    submissions: Vec<EntityId>,
    cases: Vec<EntityId>,
    escrow: Money,
    evaluations: Vec<AccountId32>,
}
#[derive(Encode)]
enum Record {
    Worker(WorkerImport),
    Project(ProjectImport),
    Deposit(DepositView),
    Withdrawal(WithdrawalView),
    Balance(AccountId32, Money),
    Issuance(Money),
    SkillRequest(SkillRequestCore),
    Policies(ScorePolicy, CompensationPolicy, u32),
    Reason(Vec<u8>),
    Catalog(CatalogKind, CatalogEntryCore, Vec<u32>, CatalogReferenceV1),
    WorkerIndex(AccountId32),
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
fn upload(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    target: ImportTarget,
    bytes: &[u8],
) -> Result<(), Error> {
    for (index, page) in bytes.chunks(8192).enumerate() {
        write(
            ext,
            bundles,
            state,
            "import_child_page",
            (
                target,
                u32::try_from(index).map_err(|_| Error::Limit)?,
                page,
            ),
        )?;
    }
    write(ext, bundles, state, "finish_child_import", target)
}
fn upload_record(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    record: &Record,
) -> Result<(), Error> {
    let bytes = record.encode();
    for (index, page) in bytes.chunks(8192).enumerate() {
        write(
            ext,
            bundles,
            state,
            "import_record_page",
            (
                u32::try_from(index).map_err(|_| Error::Limit)?,
                page,
                index + 1 == bytes.len().div_ceil(8192),
            ),
        )?;
    }
    Ok(())
}
pub(super) fn initialize(
    bundles: &Bundles,
    archive: CoreArchiveV4,
    configuration: WholeConfiguration,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    configuration.validate()?;
    let saved = &archive.checkpoint;
    if archive.version != 4
        || saved.engine_version != 4
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
    write(&mut ext, bundles, &state, "begin_import", ())?;
    let mut records = Vec::new();
    records.push(Record::Policies(
        saved.score_policy,
        saved.compensation_policy.clone(),
        saved.next_task_id,
    ));
    records.extend(saved.roles.values().cloned().map(|entry| {
        let origin = entry.content.clone();
        Record::Catalog(CatalogKind::Role, entry, Vec::new(), origin)
    }));
    for (id, entry) in &saved.skills {
        let metadata = saved.skill_metadata.get(id).ok_or(Error::InvalidState)?;
        records.push(Record::Catalog(
            CatalogKind::Skill,
            entry.clone(),
            metadata.role_ids.clone(),
            metadata.origin.clone(),
        ));
    }
    records.extend(saved.workers.values().map(|worker| {
        Record::Worker(WorkerImport {
            account: worker.account,
            content: worker.content.clone(),
            qualifications: worker.qualifications.clone(),
            mode: worker.mode,
            coordinator_eligible: worker.coordinator_eligible,
            calendar_id: worker.calendar.calendar_id,
            worker_score: worker.worker_score.clone(),
            coordinator_score: worker.coordinator_score.clone(),
        })
    }));
    for worker in saved.workers.values() {
        for _ in worker.qualifications.skill_ids.chunks(32) {
            records.push(Record::WorkerIndex(worker.account));
        }
    }
    for project in saved.projects.values() {
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
        upload_record(&mut ext, bundles, &state, &record)?;
    }
    for worker in saved.workers.values() {
        let image = (&worker.calendar, &state.directory, &state.directory, false).encode();
        upload(
            &mut ext,
            bundles,
            &state,
            ImportTarget::Calendar(worker.account),
            &image,
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
    write(&mut ext, bundles, &state, "seal_import", ())?;
    read_archive(&mut ext, bundles, &state, true)?;
    Ok((ext, encode(&state)?))
}
pub(super) fn read_archive(
    ext: &mut sp_io::TestExternalities,
    _bundles: &Bundles,
    state: &State,
    sealed: bool,
) -> Result<CoreArchiveV4, Error> {
    super::archive::read(ext, state, sealed)
}
pub(super) async fn export(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
) -> Result<CoreArchiveV4, Error> {
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
    fn staged_record_faults(
        mut ext: sp_io::TestExternalities,
        bundles: &Bundles,
        state: &State,
        record: &Record,
    ) -> sp_io::TestExternalities {
        let bytes = record.encode();
        assert!(bytes.len() > 8192);
        for (index, page, finish) in [
            (1u32, &bytes[..8192], false),
            (0u32, &bytes[..8192], true),
            (0u32, &bytes[..0], false),
        ] {
            ext = preserved(ext, |ext| {
                assert!(
                    write(
                        ext,
                        bundles,
                        state,
                        "import_record_page",
                        (index, page, finish)
                    )
                    .is_err()
                );
            });
        }
        write(
            &mut ext,
            bundles,
            state,
            "import_record_page",
            (0u32, &bytes[..8192], false),
        )
        .unwrap();
        ext = preserved(ext, |ext| {
            assert!(write(ext, bundles, state, "seal_import", ()).is_err());
        });
        for (index, page, finish) in [
            (0u32, &bytes[..8192], false),
            (1u32, &bytes[8192..8193], true),
        ] {
            ext = preserved(ext, |ext| {
                assert!(
                    write(
                        ext,
                        bundles,
                        state,
                        "import_record_page",
                        (index, page, finish)
                    )
                    .is_err()
                );
            });
        }
        for (index, page) in bytes.chunks(8192).enumerate().skip(1) {
            write(
                &mut ext,
                bundles,
                state,
                "import_record_page",
                (
                    u32::try_from(index).unwrap(),
                    page,
                    index + 1 == bytes.len().div_ceil(8192),
                ),
            )
            .unwrap();
        }
        ext
    }
    #[test]
    fn failed_staged_child_pages_and_finish_preserve_state_and_retry_then_seal() {
        let bundles = Bundles::pinned().unwrap();
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
        write(&mut ext, &bundles, &state, "begin_import", ()).unwrap();
        for id in 10..44 {
            upload_record(
                &mut ext,
                &bundles,
                &state,
                &Record::Catalog(
                    CatalogKind::Skill,
                    CatalogEntryCore {
                        id,
                        content: CatalogReferenceV1::Migration(PayloadHash::from_bytes(
                            [u8::try_from(id).unwrap(); 32],
                        )),
                        fixed: false,
                    },
                    Vec::new(),
                    CatalogReferenceV1::Migration(PayloadHash::from_bytes([5; 32])),
                ),
            )
            .unwrap();
        }
        let id = EntityId::from_bytes([51; 16]);
        // Descriptive labels no longer span pages. A historical request may
        // retain many distinct (subsequently deleted) role IDs instead.
        ext = staged_record_faults(
            ext,
            &bundles,
            &state,
            &Record::SkillRequest(SkillRequestCore {
                request_id: EntityId::from_bytes([52; 16]),
                requester: owner,
                content: CatalogReferenceV1::Migration(PayloadHash::from_bytes([77; 32])),
                role_ids: (1..2200).collect(),
                status: SkillRequestStatus::Rejected,
                skill_id: None,
            }),
        );
        upload_record(
            &mut ext,
            &bundles,
            &state,
            &Record::Worker(WorkerImport {
                account: owner,
                content: generated_contracts::provider_core::MetadataReferenceV1::Migration(
                    PayloadHash::from_bytes([71; 32]),
                ),
                qualifications: Qualifications {
                    role_ids: Vec::new(),
                    skill_ids: (10..44).collect(),
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
        let target = ImportTarget::Calendar(owner);
        ext = preserved(ext, |ext| {
            assert!(
                write(
                    ext,
                    &bundles,
                    &state,
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
        let image = (&calendar, &state.directory, &state.directory, false).encode();
        assert!(image.len() > 8192);
        write(
            &mut ext,
            &bundles,
            &state,
            "import_child_page",
            (target, 0u32, &image[..8192]),
        )
        .unwrap();
        ext = preserved(ext, |ext| {
            assert!(
                write(
                    ext,
                    &bundles,
                    &state,
                    "import_child_page",
                    (target, 0u32, &image[..8192])
                )
                .is_err()
            )
        });
        ext = preserved(ext, |ext| {
            assert!(write(ext, &bundles, &state, "finish_child_import", target).is_err())
        });
        for (index, page) in image.chunks(8192).enumerate().skip(1) {
            write(
                &mut ext,
                &bundles,
                &state,
                "import_child_page",
                (target, u32::try_from(index).unwrap(), page),
            )
            .unwrap();
        }
        write(&mut ext, &bundles, &state, "finish_child_import", target).unwrap();
        for _ in 0..2 {
            ext = preserved(ext, |ext| {
                assert!(write(ext, &bundles, &state, "seal_import", ()).is_err());
            });
            upload_record(&mut ext, &bundles, &state, &Record::WorkerIndex(owner)).unwrap();
        }
        let exported = read_archive(&mut ext, &bundles, &state, false).unwrap();
        assert_eq!(exported.checkpoint.workers[&owner].calendar, calendar);
        write(&mut ext, &bundles, &state, "seal_import", ()).unwrap();
        let _exported = read_archive(&mut ext, &bundles, &state, true).unwrap();
        ext = preserved(ext, |ext| {
            assert!(
                write(
                    ext,
                    &bundles,
                    &state,
                    "import_child_page",
                    (target, 0u32, &image[..8192])
                )
                .is_err()
            )
        });
        assert!(read_archive(&mut ext, &bundles, &state, true).is_ok());
    }
}
