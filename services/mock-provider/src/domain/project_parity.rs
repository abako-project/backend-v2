//! Frozen signed M5 trace; the isolated VM does not link this native engine.
use super::super::*;
use super::directory_parity::{shell_digest, signed_account, signed_apply};
use generated_contracts::provider_core::{CatalogCore, ReasonReferenceV1, SkillRequestCore};
use generated_contracts::*;
use parity_scale_codec::Encode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Reason = (
    EntityId,
    Option<EntityId>,
    AccountId32,
    ReasonReferenceV1,
    UnixSeconds,
    DomainEventKind,
);
pub(super) type Projection = (
    CatalogCore,
    Vec<WorkerCore>,
    Vec<SkillRequestCore>,
    Vec<ProjectCore>,
    Vec<Reason>,
);
pub(super) fn projection(state: &State) -> Projection {
    let snapshot = state.snapshot();
    let mut reasons = state
        .engine
        .reasons
        .iter()
        .map(|r| {
            (
                r.project_id,
                r.milestone_id,
                r.origin,
                r.content.clone(),
                r.occurred_at,
                r.kind,
            )
        })
        .collect::<Vec<_>>();
    reasons.sort_by_key(Encode::encode);
    (
        snapshot.catalog,
        snapshot.workers,
        state.skill_requests(state.shell.info.root_account),
        snapshot.projects,
        reasons,
    )
}
fn digest(state: &State) -> String {
    format!("{:x}", Sha256::digest(projection(state).encode()))
}
#[test]
fn canonical_signed_project_trace() -> TestResult {
    let trace: Value = serde_json::from_str(include_str!("fixtures/project-signed-v5.json"))?;
    assert_eq!(trace["schemaVersion"], 1);
    let mut state = State::new(serde_json::from_value(trace["root"].clone())?)?;
    state.set_fixture_generation(serde_json::from_value(trace["generation"].clone())?);
    assert_eq!(json!(projection(&state)), trace["initial"]);
    for (index, step) in trace["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        assert_eq!(
            signed_apply(
                &mut state,
                serde_json::from_value(step["signed"].clone())?,
                serde_json::from_value(step["inputs"].clone())?
            ),
            step["outcome"],
            "step {index}"
        );
        let inventory = state.task_state();
        inventory.validate()?;
        assert_eq!(inventory.next_task_id, state.engine.next_task_id);
        if index == 25 {
            assert_eq!(inventory.next_task_id, 2);
            assert!(!inventory.storages.is_empty());
            assert!(
                inventory
                    .storages
                    .iter()
                    .all(|record| record.storage.tasks.is_empty())
            );
        }
        assert_eq!(digest(&state), step["stateDigest"], "step {index}");
        assert_eq!(
            shell_digest(&state.shell)?,
            step["shellDigest"],
            "step {index}"
        );
    }
    assert_eq!(json!(projection(&state)), trace["final"]);
    Ok(())
}
struct Fixture {
    state: State,
    steps: Vec<Value>,
}
impl Fixture {
    fn step(
        &mut self,
        origin: u8,
        command: ProviderCommand,
        seed: u8,
    ) -> TestResult<Option<EntityId>> {
        let origin_account = signed_account(origin)?;
        let op = u8::try_from(self.steps.len().checked_add(1).ok_or("step count")?)?;
        let call = UnsignedContractCallV1::new(
            self.state.shell.info.provider_instance_id,
            OperationId::from_bytes([op; 16]),
            origin_account,
            self.state.shell.nonce(origin_account),
            UnixSeconds::new(10_000),
            command,
        )?;
        let key = subxt_signer::sr25519::Keypair::from_secret_key([origin; 32])?;
        let signature = Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
        let signed = SignedContractCallV1 { call, signature };
        let now = 100u64
            .checked_add(u64::try_from(self.steps.len())?)
            .ok_or("time")?;
        let inputs = execution::ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(now),
            seed: [seed; 32],
        };
        let outcome = signed_apply(&mut self.state, signed.clone(), inputs.clone());
        let entity = outcome
            .get("receipt")
            .and_then(|r| r.get("createdEntityId"))
            .cloned()
            .map(serde_json::from_value::<Option<EntityId>>)
            .transpose()?
            .flatten();
        self.steps.push(json!({"signed":signed,"inputs":inputs,"outcome":outcome,"stateDigest":digest(&self.state),"shellDigest":shell_digest(&self.state.shell)?}));
        Ok(entity)
    }
    fn project(&self, id: EntityId) -> TestResult<&ProjectCore> {
        self.state
            .engine
            .projects
            .get(&id)
            .ok_or_else(|| "project".into())
    }
    fn proposal(
        &self,
        id: EntityId,
    ) -> TestResult<&generated_contracts::provider_core::ProposalCore> {
        self.project(id)?
            .proposals
            .first()
            .ok_or_else(|| "proposal".into())
    }
}
fn proposal(keys: &[u32]) -> TestResult<ProposalDefinition> {
    Ok(ProposalDefinition {
        title: "Plan".into(),
        description: "Unpaid planning".into(),
        milestones: keys
            .iter()
            .map(|key| {
                let week = Week::new(2026, u8::try_from(*key / 10)?.max(1))?;
                Ok(MilestoneDefinition {
                    key: *key,
                    title: format!("Milestone {key}"),
                    window: WeekWindow::new(week, week)?,
                    coordinator_fee: Money::new(7),
                    coordinator_minutes: Minutes::new(30),
                    requirements: vec![RequirementDefinition {
                        key: 1,
                        role_id: 2,
                        skill_ids: vec![1],
                        minutes: Minutes::new(37),
                        budget: Money::new(94),
                    }],
                })
            })
            .collect::<TestResult<Vec<_>>>()?,
    })
}
#[test]
#[ignore = "explicit signed M5 trace authoring only"]
#[allow(clippy::too_many_lines)]
fn export_project_trace() -> TestResult {
    let root = signed_account(1)?;
    let generation = ProviderInstanceId::from_bytes([107; 16]);
    let mut state = State::new(root)?;
    state.set_fixture_generation(generation);
    let initial = json!(projection(&state));
    let mut fixture = Fixture {
        state,
        steps: Vec::new(),
    };
    for worker_seed in [2, 3] {
        fixture.step(
            worker_seed,
            ProviderCommand::RegisterWorker(RegisterWorkerRequest {
                display_name: format!("Candidate {worker_seed}"),
                qualifications: Qualifications {
                    role_ids: vec![3],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(600),
                    overrides: vec![],
                },
            }),
            worker_seed,
        )?;
    }
    fixture.step(
        1,
        ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
            account: signed_account(2)?,
        }),
        4,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Coordinator,
        }),
        5,
    )?;
    let create = || {
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Request".into(),
            description: "No planning fee".into(),
        })
    };
    let project_a = fixture.step(1, create(), 90)?.ok_or("project A")?;
    // The original insertion could replace retained data on an ID collision.
    fixture.step(1, create(), 90)?;
    let project_b = fixture.step(1, create(), 91)?.ok_or("project B")?;
    fixture.step(
        3,
        ProviderCommand::CreateProposal {
            project_id: project_a,
            proposal: proposal(&[10, 20])?,
        },
        20,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateProposal {
            project_id: EntityId::from_bytes([255; 16]),
            proposal: proposal(&[10, 20])?,
        },
        20,
    )?;
    let proposal_a = fixture
        .step(
            2,
            ProviderCommand::CreateProposal {
                project_id: project_a,
                proposal: proposal(&[10, 20])?,
            },
            20,
        )?
        .ok_or("proposal A")?;
    fixture.step(
        2,
        ProviderCommand::CreateProposal {
            project_id: project_a,
            proposal: proposal(&[10, 20])?,
        },
        20,
    )?;
    // Global collision errors follow native traversal: calendars, sorted Projects,
    // proposal order, then milestone/storage order. The later entity sets the code.
    fixture.step(
        5,
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Collision".into(),
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: vec![1],
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: vec![],
            },
        }),
        20,
    )?;
    fixture.step(1, create(), 20)?;
    fixture.step(
        2,
        ProviderCommand::UpdateProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            proposal: proposal(&[10, 20, 30])?,
        },
        20,
    )?;
    let storage = fixture.proposal(project_a)?.milestones[0]
        .task_storage
        .task_storage_id;
    let removed_storage = fixture.proposal(project_a)?.milestones[1]
        .task_storage
        .task_storage_id;
    let missing = EntityId::from_bytes([254; 16]);
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: missing,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: storage,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: missing,
            assignees: vec![signed_account(5)?],
        },
        30,
    )?;
    // Retired progress commands are rejection probes; API tests cover progress execution.
    let progress = TaskProgressRequest {
        status: TaskStatus::InProgress,
        logged_minutes: Minutes::new(12),
    };
    for worker_seed in [3, 1, 4] {
        fixture.step(
            worker_seed,
            ProviderCommand::UpdateTaskProgress {
                project_id: project_a,
                task_storage_id: storage,
                task_id: 1,
                progress: progress.clone(),
            },
            30,
        )?;
    }
    fixture.step(
        3,
        ProviderCommand::SetTaskAssigneesCore {
            project_id: project_a,
            task_storage_id: storage,
            task_id: 1,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetTaskAssigneesCore {
            project_id: project_a,
            task_storage_id: storage,
            task_id: 1,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::UpdateTaskProgress {
            project_id: project_a,
            task_storage_id: storage,
            task_id: 1,
            progress: progress.clone(),
        },
        30,
    )?;
    fixture.step(
        1,
        ProviderCommand::UpdateTaskProgress {
            project_id: project_a,
            task_storage_id: missing,
            task_id: 1,
            progress,
        },
        30,
    )?;
    for expected_revision in [1, 0] {
        fixture.step(
            2,
            ProviderCommand::DeleteTask {
                project_id: project_a,
                task_storage_id: storage,
                task_id: 1,
                expected_revision,
            },
            30,
        )?;
    }
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: storage,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::UpdateProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            proposal: proposal(&[10, 30])?,
        },
        21,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: removed_storage,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetProposalDelivery {
            project_id: project_a,
            proposal_id: proposal_a,
            request: ProposalDeliveryRequest {
                expected_revision: 1,
                delivery: DeliveryPreference::SpecificDate {
                    date: "2026-01-01".into(),
                },
            },
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: project_a,
            proposal_id: proposal_a,
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetProposalDelivery {
            project_id: project_a,
            proposal_id: proposal_a,
            request: ProposalDeliveryRequest {
                expected_revision: 1,
                delivery: DeliveryPreference::WithinOneMonth {},
            },
        },
        30,
    )?;
    // Two inclusive one-week durations from ISO 2026-W01 yield January 12.
    fixture.step(
        2,
        ProviderCommand::SetProposalDelivery {
            project_id: project_a,
            proposal_id: proposal_a,
            request: ProposalDeliveryRequest {
                expected_revision: 2,
                delivery: DeliveryPreference::SpecificDate {
                    date: "2026-01-12".into(),
                },
            },
        },
        30,
    )?;
    for worker_seed in [1, 2] {
        fixture.step(
            worker_seed,
            ProviderCommand::SubmitProposal {
                project_id: project_a,
                proposal_id: proposal_a,
            },
            30,
        )?;
    }
    for worker_seed in [2, 1] {
        fixture.step(
            worker_seed,
            ProviderCommand::AcceptPlanningDelivery {
                project_id: project_a,
                expected_revision: 0,
            },
            30,
        )?;
    }
    fixture.step(
        1,
        ProviderCommand::RequestProposalChanges {
            project_id: project_a,
            proposal_id: proposal_a,
            request: RequestChangesRequest {
                reference: "Need details".into(),
            },
        },
        30,
    )?;
    let mut invalid = proposal(&[10, 30])?;
    invalid.milestones[0].requirements[0].skill_ids = vec![999];
    fixture.step(
        2,
        ProviderCommand::UpdateProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            proposal: invalid,
        },
        22,
    )?;
    let revision = fixture.proposal(project_a)?.revision;
    fixture.step(
        2,
        ProviderCommand::WithdrawProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            expected_revision: revision,
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: project_a,
            proposal_id: proposal_a,
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::DeleteTask {
            project_id: project_a,
            task_storage_id: storage,
            task_id: 2,
            expected_revision: 0,
        },
        30,
    )?;
    let revision = fixture.proposal(project_a)?.revision;
    fixture.step(
        2,
        ProviderCommand::WithdrawProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            expected_revision: revision,
        },
        30,
    )?;
    let policy = CompensationPolicy {
        coordinator_percent: Percentage::new(33)?,
    };
    for worker_seed in [3, 1] {
        fixture.step(
            worker_seed,
            ProviderCommand::SetCompensationPolicy(policy.clone()),
            30,
        )?;
    }
    fixture.step(
        2,
        ProviderCommand::UpdateProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            proposal: proposal(&[10, 30])?,
        },
        23,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: project_a,
            proposal_id: proposal_a,
        },
        30,
    )?;
    let revision = fixture.project(project_a)?.planning.revision;
    fixture.step(
        1,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: project_a,
            expected_revision: revision,
        },
        30,
    )?;
    let week = Week::new(2026, 1)?;
    fixture.step(
        1,
        ProviderCommand::QuotePlanning {
            project_id: project_a,
            quote: PlanningQuote {
                fee: Money::new(100),
                minutes: Minutes::new(60),
                window: WeekWindow::new(week, week)?,
            },
        },
        30,
    )?;
    fixture.step(
        1,
        ProviderCommand::AcceptPlanningQuote {
            project_id: project_a,
            expected_revision: 0,
        },
        30,
    )?;
    let revision = fixture.proposal(project_a)?.revision;
    fixture.step(
        2,
        ProviderCommand::WithdrawProposal {
            project_id: project_a,
            proposal_id: proposal_a,
            expected_revision: revision,
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: project_a,
            proposal_id: proposal_a,
        },
        30,
    )?;
    let revision = fixture.project(project_a)?.planning.revision;
    fixture.step(
        1,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: project_a,
            expected_revision: revision,
        },
        30,
    )?;
    let mut empty = proposal(&[10])?;
    empty.milestones[0].coordinator_fee = Money::ZERO;
    empty.milestones[0].coordinator_minutes = Minutes::ZERO;
    empty.milestones[0].requirements.clear();
    let pb = fixture
        .step(
            2,
            ProviderCommand::CreateProposal {
                project_id: project_b,
                proposal: empty,
            },
            50,
        )?
        .ok_or("proposal B")?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: project_b,
            proposal_id: pb,
        },
        30,
    )?;
    let revision = fixture.project(project_b)?.planning.revision;
    fixture.step(
        1,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: project_b,
            expected_revision: revision,
        },
        30,
    )?;
    for (kind, id, name) in [
        (CatalogKind::Role, 90, "Temporary role"),
        (CatalogKind::Skill, 80, "Temporary skill"),
    ] {
        fixture.step(
            1,
            ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
                kind,
                id,
                name: name.into(),
            }),
            30,
        )?;
    }
    let mut refs = proposal(&[10])?;
    refs.milestones[0].requirements[0].role_id = 90;
    refs.milestones[0].requirements[0].skill_ids = vec![80];
    let pr = fixture
        .step(
            2,
            ProviderCommand::CreateProposal {
                project_id: project_b,
                proposal: refs,
            },
            51,
        )?
        .ok_or("reference proposal")?;
    fixture.step(
        1,
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: 80,
        }),
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::DeleteProposal {
            project_id: project_b,
            proposal_id: pr,
        },
        30,
    )?;
    for (kind, id) in [(CatalogKind::Skill, 80), (CatalogKind::Role, 90)] {
        fixture.step(
            1,
            ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest { kind, id }),
            30,
        )?;
    }
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_b,
            task_storage_id: storage,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    let sb = fixture.proposal(project_b)?.milestones[0]
        .task_storage
        .task_storage_id;
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_b,
            task_storage_id: sb,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    let project_c = fixture.step(1, create(), 92)?.ok_or("project C")?;
    for worker_seed in [3, 2] {
        fixture.step(
            worker_seed,
            ProviderCommand::DisputePlanning {
                project_id: project_c,
                request: ReasonRequest {
                    reason: "Schedule question".into(),
                },
            },
            30,
        )?;
    }
    fixture.step(
        2,
        ProviderCommand::CreateProposal {
            project_id: project_c,
            proposal: proposal(&[10])?,
        },
        52,
    )?;
    for worker_seed in [1, 2] {
        fixture.step(
            worker_seed,
            ProviderCommand::CancelProject {
                project_id: project_c,
                request: ReasonRequest {
                    reason: "No funds exchanged".into(),
                },
            },
            30,
        )?;
    }
    fixture.step(
        1,
        ProviderCommand::CancelProject {
            project_id: project_a,
            request: ReasonRequest {
                reason: "Archive plan".into(),
            },
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateTaskCore {
            project_id: project_a,
            task_storage_id: storage,
            assignees: vec![signed_account(3)?],
        },
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::DeleteProposal {
            project_id: project_a,
            proposal_id: proposal_a,
        },
        30,
    )?;
    assert_eq!(
        fixture.proposal(project_b)?.milestones[0]
            .task_storage
            .tasks[0]
            .task_id,
        3
    );
    assert_eq!(
        fixture.project(project_a)?.planning.status,
        PlanningStatus::Completed
    );
    assert!(fixture.project(project_a)?.cancelled);
    assert!(!fixture.project(project_b)?.cancelled);
    assert_eq!(fixture.state.engine.reasons.len(), 3);
    assert!(
        fixture
            .state
            .engine
            .workers
            .values()
            .all(|w| w.calendar.reservations.is_empty())
    );
    assert!(
        fixture
            .state
            .engine
            .balances
            .values()
            .all(|amount| *amount == Money::ZERO)
    );
    let accounts = (1..=5)
        .map(signed_account)
        .collect::<TestResult<Vec<_>>>()?;
    let final_state = json!(projection(&fixture.state));
    std::fs::write(
        std::env::var("PROJECT_SIGNED_TRACE_OUTPUT")?,
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":1,"root":root,"generation":generation,"accounts":accounts,"initial":initial,"steps":fixture.steps,"final":final_state}),
        )?,
    )?;
    Ok(())
}
