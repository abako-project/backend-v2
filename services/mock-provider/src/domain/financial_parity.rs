//! Signed financial reference trace; replayed against production v5 WASM.
use super::super::*;
use super::directory_parity::{shell_digest, signed_account, signed_apply};
use generated_contracts::*;
use parity_scale_codec::Encode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type BankProjection = (
    Vec<(AccountId32, Money)>,
    Money,
    Vec<DepositView>,
    Vec<WithdrawalView>,
);
type Projection = (
    super::project_parity::Projection,
    BankProjection,
    Vec<(Dispute, MilestoneView)>,
);
fn projection(state: &State) -> TestResult<Projection> {
    let (deposits, withdrawals) = state.engine.bramp.projection();
    let cases = state
        .engine
        .disputes
        .keys()
        .map(|id| {
            state
                .engine
                .dispute_view(*id)
                .map(|view| (view.dispute, view.milestone))
        })
        .collect::<crate::Result<Vec<_>>>()?;
    Ok((
        super::project_parity::projection(state),
        (
            state
                .engine
                .balances
                .iter()
                .map(|(account, amount)| (*account, *amount))
                .collect(),
            state.engine.minted_units,
            deposits,
            withdrawals,
        ),
        cases,
    ))
}

fn digest(state: &State) -> TestResult<String> {
    Ok(format!("{:x}", Sha256::digest(projection(state)?.encode())))
}
#[test]
fn canonical_signed_financial_trace() -> TestResult {
    let trace: Value = serde_json::from_str(include_str!("fixtures/financial-signed.json"))?;
    let mut state = State::new(serde_json::from_value(trace["root"].clone())?)?;
    state.shell.info.provider_instance_id = serde_json::from_value(trace["generation"].clone())?;
    assert_eq!(json!(projection(&state)?), trace["initial"]);
    for (index, step) in trace["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        for probe in trace["temporalProbes"]
            .as_array()
            .ok_or("temporal probes")?
            .iter()
            .filter(|probe| probe["beforeStep"] == index)
        {
            let before_domain = digest(&state)?;
            let before_shell = shell_digest(&state.shell)?;
            assert_eq!(
                signed_apply(
                    &mut state,
                    serde_json::from_value(probe["signed"].clone())?,
                    serde_json::from_value(probe["inputs"].clone())?
                ),
                probe["outcome"],
                "temporal probe {index}"
            );
            assert_eq!(digest(&state)?, before_domain);
            assert_eq!(shell_digest(&state.shell)?, before_shell);
        }
        assert_eq!(
            signed_apply(
                &mut state,
                serde_json::from_value(step["signed"].clone())?,
                serde_json::from_value(step["inputs"].clone())?
            ),
            step["outcome"],
            "step {index}"
        );
        assert_eq!(digest(&state)?, step["stateDigest"], "step {index}");
        assert_eq!(
            shell_digest(&state.shell)?,
            step["shellDigest"],
            "step {index}"
        );
    }
    assert_eq!(json!(projection(&state)?), trace["final"]);
    Ok(())
}
struct Fixture {
    state: State,
    steps: Vec<Value>,
    temporal_probes: Vec<Value>,
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
        let mut inputs = execution::ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(now),
            seed: [seed; 32],
        };
        let mut before = self.state.clone();
        let backward = execution::ExecutionInputs {
            now: UnixSeconds::new(1),
            ..inputs.clone()
        };
        let probe = signed_apply(&mut before, signed.clone(), backward.clone());
        if matches!(
            probe["error"].as_str(),
            Some(
                "invalid_deposit_state"
                    | "invalid_withdrawal_state"
                    | "invalid_review_state"
                    | "invalid_dispute_response"
            )
        ) {
            assert_eq!(digest(&before)?, digest(&self.state)?);
            assert_eq!(
                shell_digest(&before.shell)?,
                shell_digest(&self.state.shell)?
            );
            self.temporal_probes.push(json!({"beforeStep":self.steps.len(), "signed":signed,"inputs":backward,"outcome":probe}));
        } else if probe["receipt"]["outcome"]["type"] == "Failed" {
            // Backward clocks must preserve earlier business-error priority too.
            inputs = backward;
        }
        let outcome = signed_apply(&mut self.state, signed.clone(), inputs.clone());
        let entity = outcome
            .get("receipt")
            .and_then(|r| r.get("createdEntityId"))
            .cloned()
            .map(serde_json::from_value::<Option<EntityId>>)
            .transpose()?
            .flatten();
        self.steps.push(json!({"signed":signed,"inputs":inputs,"outcome":outcome,"stateDigest":digest(&self.state)?,"shellDigest":shell_digest(&self.state.shell)?}));
        Ok(entity)
    }
}

#[test]
#[ignore = "Trace authoring requires an explicit output path"]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the ordered signed trace authoring scenario explicit."
)]
fn export_financial_trace() -> TestResult {
    let output = std::env::var("FINANCIAL_SIGNED_TRACE_OUTPUT")?;
    let mut state = State::new(signed_account(1)?)?;
    state.shell.info.provider_instance_id = ProviderInstanceId::from_bytes([109; 16]);
    let initial = projection(&state)?;
    let mut fixture = Fixture {
        state,
        steps: Vec::new(),
        temporal_probes: Vec::new(),
    };
    let amount = |units| KvnAmount::new(Money::new(units));
    let fund = |account, units| -> TestResult<_> {
        Ok(ProviderCommand::FundAccount(FundAccountRequest {
            account: signed_account(account)?,
            amount: Money::new(units),
        }))
    };
    fixture.step(1, fund(2, 37)?, 1)?;
    fixture.step(3, fund(3, 99)?, 2)?;
    fixture.step(1, fund(4, 0)?, 3)?;
    let deposit = fixture
        .step(
            4,
            ProviderCommand::CreateDeposit(CreateDepositRequest {
                amount: amount(101)?,
            }),
            30,
        )?
        .ok_or("deposit")?;
    fixture.step(
        2,
        ProviderCommand::ConfirmDeposit {
            deposit_id: deposit,
        },
        4,
    )?;
    fixture.step(
        1,
        ProviderCommand::ConfirmDeposit {
            deposit_id: deposit,
        },
        5,
    )?;
    fixture.step(
        1,
        ProviderCommand::ConfirmDeposit {
            deposit_id: deposit,
        },
        6,
    )?;
    fixture.step(
        1,
        ProviderCommand::ConfirmDeposit {
            deposit_id: EntityId::from_bytes([255; 16]),
        },
        7,
    )?;
    let withdrawal = fixture
        .step(
            4,
            ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
                amount: amount(101)?,
            }),
            31,
        )?
        .ok_or("withdrawal")?;
    fixture.step(
        4,
        ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest { amount: amount(1)? }),
        32,
    )?;
    fixture.step(
        3,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: withdrawal,
        },
        8,
    )?;
    fixture.step(
        4,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: withdrawal,
        },
        9,
    )?;
    fixture.step(
        4,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: withdrawal,
        },
        10,
    )?;
    let second = fixture
        .step(
            4,
            ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
                amount: amount(100)?,
            }),
            32,
        )?
        .ok_or("second withdrawal")?;
    fixture.step(
        1,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: second,
        },
        11,
    )?;
    let huge = fixture
        .step(
            2,
            ProviderCommand::CreateDeposit(CreateDepositRequest {
                amount: amount(u64::MAX)?,
            }),
            33,
        )?
        .ok_or("huge deposit")?;
    fixture.step(1, ProviderCommand::ConfirmDeposit { deposit_id: huge }, 12)?;
    fixture.step(
        2,
        ProviderCommand::CreateDeposit(CreateDepositRequest { amount: amount(7)? }),
        30,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest { amount: amount(7)? }),
        31,
    )?;
    fixture.step(1, fund(5, u64::MAX)?, 13)?;
    fixture.step(
        5,
        ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest { amount: amount(1)? }),
        34,
    )?;
    fixture.step(
        1,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: EntityId::from_bytes([255; 16]),
        },
        14,
    )?;
    assert_eq!(fixture.state.engine.minted_units, Money::new(138));
    execution_scenario(&mut fixture)?;
    let trace = json!({"schemaVersion":1,"root":signed_account(1)?,"generation":fixture.state.shell.info.provider_instance_id,"initial":initial,"steps":fixture.steps,"temporalProbes":fixture.temporal_probes,"final":projection(&fixture.state)?});
    std::fs::write(output, serde_json::to_vec_pretty(&trace)?)?;
    Ok(())
}

fn definition(milestones: u8, requirements: u8, budget: u64) -> TestResult<ProposalDefinition> {
    Ok(ProposalDefinition {
        title: "Captured payout shares".into(),
        description: "Tracking is optional".into(),
        milestones: (1..=milestones)
            .map(|index| {
                let week = Week::new(2026, index)?;
                let fee = if budget == 0 { 0 } else { 7 };
                let remainder = budget
                    .checked_sub(fee)
                    .ok_or("budget smaller than fixture fee")?;
                Ok(MilestoneDefinition {
                    key: u32::from(index),
                    title: format!("Stage {index}"),
                    window: WeekWindow::new(week, week)?,
                    coordinator_fee: Money::new(fee),
                    coordinator_minutes: Minutes::new(if budget == 0 { 0 } else { 30 }),
                    requirements: (1..=requirements)
                        .map(|key| RequirementDefinition {
                            key: u32::from(key),
                            role_id: 2,
                            skill_ids: vec![1],
                            minutes: Minutes::new(37),
                            budget: Money::new(if key == 1 { remainder } else { 0 }),
                        })
                        .collect(),
                })
            })
            .collect::<TestResult<Vec<_>>>()?,
    })
}
fn project(fixture: &Fixture, id: EntityId) -> TestResult<&ProjectView> {
    fixture
        .state
        .engine
        .projects
        .get(&id)
        .ok_or_else(|| "project".into())
}
fn prepared(
    fixture: &mut Fixture,
    seed: u8,
    proposal_seed: u8,
    milestones: u8,
    requirements: u8,
    budget: u64,
) -> TestResult<(EntityId, EntityId)> {
    let id = fixture
        .step(
            4,
            ProviderCommand::CreateProject(CreateProjectRequest {
                title: "Financial trace".into(),
                description: "Unpaid planning".into(),
            }),
            seed,
        )?
        .ok_or("project")?;
    let proposal = fixture
        .step(
            2,
            ProviderCommand::CreateProposal {
                project_id: id,
                proposal: definition(milestones, requirements, budget)?,
            },
            proposal_seed,
        )?
        .ok_or("proposal")?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: id,
            proposal_id: proposal,
        },
        seed,
    )?;
    fixture.step(
        4,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: id,
            expected_revision: 1,
        },
        seed,
    )?;
    Ok((id, proposal))
}
fn submit(
    fixture: &mut Fixture,
    id: EntityId,
    milestone: EntityId,
    seed: u8,
) -> TestResult<EntityId> {
    fixture
        .step(
            2,
            ProviderCommand::SubmitMilestoneDelivery {
                project_id: id,
                milestone_id: milestone,
                request: SubmitMilestoneCompletionRequest { deliverable: None },
            },
            seed,
        )?
        .ok_or_else(|| "submission".into())
}
fn accept(id: EntityId, milestone: EntityId, submission: EntityId) -> ProviderCommand {
    ProviderCommand::AcceptMilestoneDelivery {
        project_id: id,
        milestone_id: milestone,
        submission_id: submission,
    }
}
fn evidence() -> TestResult<EvidenceReference> {
    Ok(EvidenceReference::new("https://example.org/case".into())?)
}
#[allow(
    clippy::too_many_lines,
    reason = "Preserve the ordered lifecycle and its captured-ID regressions in one scenario."
)]
fn execution_scenario(fixture: &mut Fixture) -> TestResult {
    for (origin, seed) in [(2, 40), (3, 41), (5, 42)] {
        fixture.step(
            origin,
            ProviderCommand::RegisterWorker(RegisterWorkerRequest {
                display_name: format!("Worker {origin}"),
                qualifications: Qualifications {
                    role_ids: vec![3],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(600),
                    overrides: vec![],
                },
            }),
            seed,
        )?;
    }
    fixture.step(
        1,
        ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
            account: signed_account(2)?,
        }),
        43,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Coordinator,
        }),
        44,
    )?;
    // Funding and earlier milestone reservations must roll back when a later
    // slot cannot fit. Preserve this pending proposal and its captured prices.
    let (unfilled, unfilled_proposal) = prepared(fixture, 79, 99, 2, 2, 11)?;
    fixture.step(
        2,
        ProviderCommand::WithdrawProposal {
            project_id: unfilled,
            proposal_id: unfilled_proposal,
            expected_revision: 1,
        },
        97,
    )?;
    let mut impossible = definition(2, 2, 11)?;
    impossible.milestones[1].requirements[0].minutes = Minutes::new(10000);
    fixture.step(
        2,
        ProviderCommand::UpdateProposal {
            project_id: unfilled,
            proposal_id: unfilled_proposal,
            proposal: impossible,
        },
        98,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: unfilled,
            proposal_id: unfilled_proposal,
        },
        99,
    )?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: unfilled,
            proposal_id: unfilled_proposal,
            expected_revision: 4,
        },
        100,
    )?;
    let (happy, proposal) = prepared(fixture, 80, 100, 2, 2, 101)?;
    let approve = ProviderCommand::ApproveExecution {
        project_id: happy,
        proposal_id: proposal,
        expected_revision: 1,
    };
    fixture.step(4, approve.clone(), 45)?; // insufficient free balance; no reservation or asset deposit
    fixture.step(
        1,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: signed_account(4)?,
            amount: Money::new(101),
        }),
        46,
    )?;
    fixture.step(2, approve.clone(), 47)?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: happy,
            proposal_id: proposal,
            expected_revision: 0,
        },
        48,
    )?;
    fixture.step(4, approve.clone(), 49)?;
    fixture.step(4, approve, 50)?;
    fixture.step(
        1,
        ProviderCommand::SetCompensationPolicy(CompensationPolicy {
            coordinator_percent: Percentage::new(33)?,
        }),
        92,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Worker,
        }),
        93,
    )?;

    let first = project(fixture, happy)?.proposals[0].milestones[0].milestone_id;
    let second = project(fixture, happy)?.proposals[0].milestones[1].milestone_id;
    let storage = project(fixture, happy)?.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    fixture.step(
        2,
        ProviderCommand::CreateTask {
            project_id: happy,
            task_storage_id: storage,
            task: TaskDefinition {
                title: "Tracking does not change allocation".into(),
                description: String::new(),
                task_type: TaskType::Task,
                priority: TaskPriority::Medium,
                status: TaskStatus::ToDo,
                assignees: vec![signed_account(2)?],
                estimated_minutes: Minutes::ZERO,
                logged_minutes: Minutes::new(99999),
                due_at: None,
            },
        },
        51,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitMilestoneDelivery {
            project_id: happy,
            milestone_id: second,
            request: SubmitMilestoneCompletionRequest { deliverable: None },
        },
        52,
    )?;
    fixture.step(
        4,
        ProviderCommand::EvaluateProject {
            project_id: happy,
            request: EvaluateProjectRequest {
                ratings: vec![AccountRating {
                    account: signed_account(2)?,
                    score: Score::new(5)?,
                }],
            },
        },
        53,
    )?;
    fixture.step(
        3,
        ProviderCommand::SubmitMilestoneDelivery {
            project_id: happy,
            milestone_id: first,
            request: SubmitMilestoneCompletionRequest { deliverable: None },
        },
        54,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitMilestoneDelivery {
            project_id: happy,
            milestone_id: first,
            request: SubmitMilestoneCompletionRequest { deliverable: None },
        },
        30,
    )?; // bank-ID collision is an invariant, not ordinary Bramp rejection
    let submission = submit(fixture, happy, first, 120)?;
    fixture.step(3, accept(happy, first, submission), 55)?;
    fixture.step(4, accept(happy, first, EntityId::from_bytes([255; 16])), 56)?;
    fixture.step(
        4,
        ProviderCommand::OpenDisputeWithComment(OpenDisputeWithCommentRequest {
            project_id: happy,
            milestone_id: first,
            rejected_submission_id: submission,
            comment_id: EntityId::from_bytes([150; 16]),
        }),
        57,
    )?;
    fixture.step(
        4,
        ProviderCommand::RejectMilestoneDelivery {
            project_id: happy,
            milestone_id: first,
            submission_id: submission,
            comment_id: EntityId::from_bytes([151; 16]),
        },
        58,
    )?;
    let revised = submit(fixture, happy, first, 121)?;
    fixture.step(
        4,
        ProviderCommand::OpenDisputeWithComment(OpenDisputeWithCommentRequest {
            project_id: happy,
            milestone_id: first,
            rejected_submission_id: submission,
            comment_id: EntityId::from_bytes([150; 16]),
        }),
        59,
    )?;
    fixture.step(4, accept(happy, first, revised), 60)?;
    fixture.step(4, accept(happy, first, revised), 61)?;
    let legacy_reference: EvidenceReference = serde_json::from_value(
        json!({"url":"https://例え.テスト/deliverable","sha256":format!("0x{}","11".repeat(32))}),
    )?;
    let final_submission = fixture
        .step(
            2,
            ProviderCommand::SubmitMilestoneDelivery {
                project_id: happy,
                milestone_id: second,
                request: SubmitMilestoneCompletionRequest {
                    deliverable: Some(legacy_reference),
                },
            },
            122,
        )?
        .ok_or("final submission")?;
    fixture.step(4, accept(happy, second, final_submission), 62)?;
    fixture.step(
        4,
        ProviderCommand::EvaluateProject {
            project_id: happy,
            request: EvaluateProjectRequest {
                ratings: vec![AccountRating {
                    account: signed_account(3)?,
                    score: Score::new(5)?,
                }],
            },
        },
        63,
    )?;
    for (author, target, stars) in [(4, 2, 5), (3, 2, 4), (1, 2, 5)] {
        fixture.step(
            author,
            ProviderCommand::EvaluateProject {
                project_id: happy,
                request: EvaluateProjectRequest {
                    ratings: vec![AccountRating {
                        account: signed_account(target)?,
                        score: Score::new(stars)?,
                    }],
                },
            },
            64,
        )?;
    }
    fixture.step(
        1,
        ProviderCommand::SetScorePolicy(
            ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?
                .with_coordinator_vote_weights(2, 3)?,
        ),
        65,
    )?;
    fixture.step(
        5,
        ProviderCommand::EvaluateProject {
            project_id: happy,
            request: EvaluateProjectRequest {
                ratings: vec![AccountRating {
                    account: signed_account(2)?,
                    score: Score::new(2)?,
                }],
            },
        },
        66,
    )?;
    fixture.step(
        2,
        ProviderCommand::EvaluateProject {
            project_id: happy,
            request: EvaluateProjectRequest {
                ratings: vec![
                    AccountRating {
                        account: signed_account(4)?,
                        score: Score::new(3)?,
                    },
                    AccountRating {
                        account: signed_account(3)?,
                        score: Score::new(4)?,
                    },
                    AccountRating {
                        account: signed_account(5)?,
                        score: Score::new(2)?,
                    },
                ],
            },
        },
        67,
    )?;
    fixture.step(
        4,
        ProviderCommand::EvaluateProject {
            project_id: happy,
            request: EvaluateProjectRequest {
                ratings: vec![AccountRating {
                    account: signed_account(2)?,
                    score: Score::new(5)?,
                }],
            },
        },
        68,
    )?;
    fixture.step(
        2,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Coordinator,
        }),
        94,
    )?;
    assert!(project(fixture, happy)?.completed);
    assert_eq!(project(fixture, happy)?.execution_escrow, Money::ZERO);

    fixture.step(
        1,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: signed_account(4)?,
            amount: Money::new(22),
        }),
        69,
    )?;
    let (legacy, legacy_proposal) = prepared(fixture, 81, 101, 2, 1, 11)?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: legacy,
            proposal_id: legacy_proposal,
            expected_revision: 1,
        },
        70,
    )?;
    let legacy_milestone = &project(fixture, legacy)?.proposals[0].milestones[0];
    let legacy_milestone_id = legacy_milestone.milestone_id;
    let worker = legacy_milestone.assignments[0].worker;
    let legacy_delivery_id = fixture
        .step(
            2,
            ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
                project_id: legacy,
                milestone_id: legacy_milestone_id,
                worker_ratings: vec![WorkerRating {
                    worker,
                    score: Score::new(4)?,
                }],
            },
            123,
        )?
        .ok_or("legacy submission")?;
    fixture.step(
        4,
        ProviderCommand::RejectMilestoneCompletion {
            project_id: legacy,
            milestone_id: legacy_milestone_id,
            submission_id: legacy_delivery_id,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        95,
    )?;
    let legacy_revised = fixture
        .step(
            2,
            ProviderCommand::RequestMilestoneCompletion {
                project_id: legacy,
                milestone_id: legacy_milestone_id,
                request: RequestMilestoneCompletionRequest {
                    worker_ratings: vec![WorkerRating {
                        worker,
                        score: Score::new(5)?,
                    }],
                    deliverable: evidence()?,
                },
            },
            129,
        )?
        .ok_or("legacy revised delivery")?;
    fixture.step(
        4,
        ProviderCommand::AcceptMilestoneCompletion {
            project_id: legacy,
            milestone_id: legacy_milestone_id,
            request: AcceptMilestoneCompletionRequest {
                submission_id: legacy_revised,
                coordinator_score: Score::new(3)?,
                team_rating: TeamRating::Client(Score::new(2)?),
            },
        },
        71,
    )?;

    let legacy_second = &project(fixture, legacy)?.proposals[0].milestones[1];
    let legacy_next = legacy_second.milestone_id;
    let next_worker = legacy_second.assignments[0].worker;
    let delegated = fixture
        .step(
            2,
            ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
                project_id: legacy,
                milestone_id: legacy_next,
                worker_ratings: vec![WorkerRating {
                    worker: next_worker,
                    score: Score::new(4)?,
                }],
            },
            128,
        )?
        .ok_or("delegated delivery")?;
    fixture.step(
        4,
        ProviderCommand::AcceptMilestoneCompletion {
            project_id: legacy,
            milestone_id: legacy_next,
            request: AcceptMilestoneCompletionRequest {
                submission_id: delegated,
                coordinator_score: Score::new(4)?,
                team_rating: TeamRating::DelegateToCoordinator,
            },
        },
        96,
    )?;
    fixture.step(
        1,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: signed_account(4)?,
            amount: Money::new(202),
        }),
        72,
    )?;
    let (frozen, frozen_proposal) = prepared(fixture, 82, 102, 2, 2, 101)?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: frozen,
            proposal_id: frozen_proposal,
            expected_revision: 1,
        },
        73,
    )?;
    let frozen_milestone_id = project(fixture, frozen)?.proposals[0].milestones[0].milestone_id;
    let frozen_delivery_id = submit(fixture, frozen, frozen_milestone_id, 124)?;
    fixture.step(
        4,
        ProviderCommand::RejectMilestoneDelivery {
            project_id: frozen,
            milestone_id: frozen_milestone_id,
            submission_id: frozen_delivery_id,
            comment_id: EntityId::from_bytes([160; 16]),
        },
        74,
    )?;
    let opening = ProviderCommand::OpenDisputeWithComment(OpenDisputeWithCommentRequest {
        project_id: frozen,
        milestone_id: frozen_milestone_id,
        rejected_submission_id: frozen_delivery_id,
        comment_id: EntityId::from_bytes([161; 16]),
    });
    fixture.step(3, opening.clone(), 150)?;
    fixture.step(4, opening.clone(), 120)?; // submission-ID collision
    let case = fixture.step(4, opening.clone(), 150)?.ok_or("case")?;
    fixture.step(
        2,
        ProviderCommand::CancelProject {
            project_id: frozen,
            request: ReasonRequest {
                reason: "Cannot unlock".into(),
            },
        },
        75,
    )?;
    fixture.step(
        4,
        accept(frozen, frozen_milestone_id, frozen_delivery_id),
        76,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitProposal {
            project_id: frozen,
            proposal_id: frozen_proposal,
        },
        77,
    )?;
    fixture.step(
        2,
        ProviderCommand::SubmitMilestoneDelivery {
            project_id: frozen,
            milestone_id: frozen_milestone_id,
            request: SubmitMilestoneCompletionRequest { deliverable: None },
        },
        125,
    )?;
    fixture.step(
        4,
        ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
            amount: KvnAmount::new(Money::new(1))?,
        }),
        78,
    )?;
    for author in [4, 3, 1] {
        fixture.step(
            author,
            ProviderCommand::RespondDispute {
                project_id: frozen,
                dispute_id: case,
                request: EvidenceRequest {
                    evidence: evidence()?,
                },
            },
            79,
        )?;
    }
    fixture.step(
        2,
        ProviderCommand::RespondDispute {
            project_id: frozen,
            dispute_id: case,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        80,
    )?;
    fixture.step(
        2,
        ProviderCommand::RespondDispute {
            project_id: frozen,
            dispute_id: case,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        81,
    )?;
    fixture.step(4, opening, 151)?;
    fixture.step(
        2,
        ProviderCommand::UpdateTaskProgress {
            project_id: happy,
            task_storage_id: storage,
            task_id: 1,
            progress: TaskProgressRequest {
                status: TaskStatus::Done,
                logged_minutes: Minutes::new(1),
            },
        },
        82,
    )?;
    fixture.step(
        2,
        ProviderCommand::CreateDeposit(CreateDepositRequest {
            amount: KvnAmount::new(Money::new(7))?,
        }),
        120,
    )?; // global later-submission collision

    let (zero, zero_proposal) = prepared(fixture, 83, 103, 1, 0, 0)?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: zero,
            proposal_id: zero_proposal,
            expected_revision: 1,
        },
        83,
    )?;
    let zero_milestone_id = project(fixture, zero)?.proposals[0].milestones[0].milestone_id;
    let zero_delivery_id = submit(fixture, zero, zero_milestone_id, 126)?;
    fixture.step(
        4,
        ProviderCommand::RejectMilestoneCompletion {
            project_id: zero,
            milestone_id: zero_milestone_id,
            submission_id: zero_delivery_id,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        84,
    )?;
    fixture.step(
        2,
        ProviderCommand::OpenDispute(OpenDisputeRequest {
            project_id: zero,
            milestone_id: zero_milestone_id,
            rejected_submission_id: zero_delivery_id,
            evidence: evidence()?,
        }),
        150,
    )?;
    let zero_case = fixture
        .step(
            2,
            ProviderCommand::OpenDispute(OpenDisputeRequest {
                project_id: zero,
                milestone_id: zero_milestone_id,
                rejected_submission_id: zero_delivery_id,
                evidence: evidence()?,
            }),
            151,
        )?
        .ok_or("zero case")?;
    fixture.step(
        4,
        ProviderCommand::RespondDispute {
            project_id: zero,
            dispute_id: zero_case,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        85,
    )?;
    fixture.step(
        2,
        ProviderCommand::RespondDispute {
            project_id: happy,
            dispute_id: case,
            request: EvidenceRequest {
                evidence: evidence()?,
            },
        },
        86,
    )?;

    fixture.step(
        1,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: signed_account(4)?,
            amount: Money::new(202),
        }),
        87,
    )?;
    let (cancelled, cancelled_proposal) = prepared(fixture, 84, 104, 2, 2, 101)?;
    fixture.step(
        4,
        ProviderCommand::ApproveExecution {
            project_id: cancelled,
            proposal_id: cancelled_proposal,
            expected_revision: 1,
        },
        88,
    )?;
    let cancelled_milestone_id =
        project(fixture, cancelled)?.proposals[0].milestones[0].milestone_id;
    let cancelled_delivery_id = submit(fixture, cancelled, cancelled_milestone_id, 127)?;
    fixture.step(
        4,
        accept(cancelled, cancelled_milestone_id, cancelled_delivery_id),
        89,
    )?;
    fixture.step(
        2,
        ProviderCommand::CancelProject {
            project_id: cancelled,
            request: ReasonRequest {
                reason: "Keep paid history and freeze remainder".into(),
            },
        },
        90,
    )?;
    fixture.step(
        4,
        accept(cancelled, cancelled_milestone_id, cancelled_delivery_id),
        91,
    )?;
    assert_eq!(
        project(fixture, cancelled)?.execution_escrow,
        Money::new(101)
    );
    assert_eq!(project(fixture, frozen)?.execution_escrow, Money::new(202));
    assert_eq!(fixture.state.engine.disputes.len(), 2);
    Ok(())
}

#[test]
fn complete_private_archive_roundtrips_history_and_current_writes() -> TestResult {
    let trace: Value = serde_json::from_str(include_str!("fixtures/financial-signed.json"))?;
    let root = serde_json::from_value(trace["root"].clone())?;
    let mut state = State::new(root)?;
    state.shell.info.provider_instance_id = serde_json::from_value(trace["generation"].clone())?;
    for (index, step) in trace["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        let before = state.encode()?;
        // Typed decoding is essential: retained reputation can exceed JSON's u64 range.
        let archive = serde_json::from_slice(&serde_json::to_vec(&state.archive()?)?)?;
        state = State::from_archive(archive, root)?;
        assert_eq!(state.encode()?, before, "archive before step {index}");
        assert_eq!(
            signed_apply(
                &mut state,
                serde_json::from_value(step["signed"].clone())?,
                serde_json::from_value(step["inputs"].clone())?
            ),
            step["outcome"],
            "step {index}"
        );
        assert_eq!(digest(&state)?, step["stateDigest"], "step {index}");
        assert_eq!(
            shell_digest(&state.shell)?,
            step["shellDigest"],
            "step {index}"
        );
    }
    let worker = state.engine.workers.values_mut().next().ok_or("worker")?;
    worker.worker_score.rated_minutes = u64::MAX;
    worker.worker_score.weighted_score_sum = u128::from(u64::MAX) * 500;
    // Deleted tasks leave a counter that cannot be reconstructed from present tasks.
    state.engine.next_task_id = 1_000_000;
    let before = state.encode()?;
    let archive = serde_json::from_slice(&serde_json::to_vec(&state.archive()?)?)?;
    let restored = State::from_archive(archive, root)?;
    assert_eq!(restored.encode()?, before);
    for stale_counter in [0, 1] {
        let mut invalid = restored.archive()?;
        invalid.checkpoint.next_task_id = stale_counter;
        assert_eq!(
            State::from_archive(invalid, root)
                .err()
                .ok_or("stale counter accepted")?
                .code(),
            "invalid_task_state"
        );
    }
    let mut invalid = restored.archive()?;
    let storage = invalid
        .checkpoint
        .projects
        .values_mut()
        .flat_map(|project| &mut project.proposals)
        .flat_map(|proposal| &mut proposal.milestones)
        .map(|milestone| &mut milestone.task_storage)
        .find(|storage| !storage.tasks.is_empty())
        .ok_or("task storage")?;
    storage.tasks.push(storage.tasks[0].clone());
    assert_eq!(
        State::from_archive(invalid, root)
            .err()
            .ok_or("duplicate task key accepted")?
            .code(),
        "invalid_task_state"
    );
    let mut invalid = restored.archive()?;
    invalid.version = 2;
    assert_eq!(
        State::from_archive(invalid, root)
            .err()
            .ok_or("version accepted")?
            .code(),
        "state_configuration_mismatch"
    );
    let mut invalid = restored.archive()?;
    invalid.checkpoint.minted_units = Money::ZERO;
    assert_eq!(
        State::from_archive(invalid, root)
            .err()
            .ok_or("supply accepted")?
            .code(),
        "supply_mismatch"
    );
    assert_eq!(
        State::from_archive(restored.archive()?, AccountId32::from_bytes([0; 32]))
            .err()
            .ok_or("root accepted")?
            .code(),
        "state_configuration_mismatch"
    );
    Ok(())
}

#[test]
fn native_accepts_large_worker_profile_payload() -> TestResult {
    let mut state = State::new(signed_account(1)?)?;
    state.shell.info.provider_instance_id = ProviderInstanceId::from_bytes([109; 16]);
    let mut fixture = Fixture {
        state,
        steps: Vec::new(),
        temporal_probes: Vec::new(),
    };
    fixture.step(
        2,
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "n".repeat(17 * 1024),
            qualifications: Qualifications {
                role_ids: Vec::new(),
                skill_ids: Vec::new(),
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: Vec::new(),
            },
        }),
        1,
    )?;
    assert_eq!(
        fixture.steps[0]["outcome"]["receipt"]["outcome"]["type"],
        "Success"
    );
    let restored = State::from_archive(fixture.state.archive()?, signed_account(1)?)?;
    assert_eq!(restored.encode()?, fixture.state.encode()?);
    Ok(())
}
