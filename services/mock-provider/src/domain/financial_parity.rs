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
    Vec<(Dispute, generated_contracts::provider_core::MilestoneCore)>,
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
fn project(fixture: &Fixture, id: EntityId) -> TestResult<&ProjectCore> {
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
    invalid.version = 4;
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
    for length in [17 * 1024, MAX_SIGNABLE_BYTES - 200] {
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
                display_name: "n".repeat(length),
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
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MigrationReference {
    archives: Vec<RetainedArchive>,
    digests: Vec<String>,
    priority_cases: Vec<serde_json::Value>,
    aggregate_cases: Vec<Value>,
    planning_cases: Vec<Value>,
}
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedArchive {
    before_step: usize,
    archive: String,
}
fn retained_priority_cases(
    state: &State,
    submitted: &SignedContractCallV1,
    frozen: Option<&[Value]>,
) -> TestResult<Vec<Value>> {
    let mut cases = Vec::new();
    let ProviderCommand::SubmitMilestoneDelivery {
        project_id,
        milestone_id,
        ..
    } = submitted.call.command
    else {
        return Err("modern submission fixture".into());
    };
    let project = state.engine.projects.get(&project_id).ok_or("project")?;
    let coordinator = project.coordinator;
    let client = project.client;
    let submission = project
        .proposals
        .iter()
        .flat_map(|p| &p.milestones)
        .find(|m| m.milestone_id == milestone_id)
        .and_then(|m| m.submissions.last())
        .ok_or("submission")?
        .submission_id;
    let client_seed = (1..=5)
        .find(|seed| signed_account(*seed).ok() == Some(client))
        .ok_or("client key")?;
    for (index, overflow) in [false, true].into_iter().enumerate() {
        let mut archived = state.archive()?;
        archived.checkpoint.next_task_id = 1_000_000;
        if overflow {
            let worker = archived
                .checkpoint
                .workers
                .get_mut(&coordinator)
                .ok_or("coordinator")?;
            worker.coordinator_score = ReputationView {
                rated_minutes: u64::MAX,
                weighted_score_sum: u128::from(u64::MAX) * 500,
            };
        }
        let archive = serde_json::to_string(&archived)?;
        let mut restored = State::from_archive(archived, state.shell.info.root_account)?;
        let call = UnsignedContractCallV1::new(
            restored.shell.info.provider_instance_id,
            OperationId::from_bytes([230; 16]),
            client,
            restored.shell.nonce(client),
            UnixSeconds::new(10_000),
            ProviderCommand::AcceptMilestoneCompletion {
                project_id,
                milestone_id,
                request: AcceptMilestoneCompletionRequest {
                    coordinator_score: Score::new(5)?,
                    team_rating: TeamRating::DelegateToCoordinator,
                    submission_id: submission,
                },
            },
        )?;
        let signed = if let Some(frozen) = frozen {
            let stored: SignedContractCallV1 = serde_json::from_value(
                frozen.get(index).ok_or("priority case")?["signed"].clone(),
            )?;
            assert_eq!(stored.call, call);
            stored
        } else {
            let key = subxt_signer::sr25519::Keypair::from_secret_key([client_seed; 32])?;
            SignedContractCallV1 {
                signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
                call,
            }
        };
        let inputs = execution::ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(200),
            seed: [230; 32],
        };
        let outcome = signed_apply(&mut restored, signed.clone(), inputs.clone());
        if overflow {
            assert_eq!(outcome["receipt"]["outcome"]["code"], "score_overflow");
        } else {
            assert_eq!(outcome["error"], "provider_unavailable");
        }
        cases.push(json!({"archive":archive,"signed":signed,"inputs":inputs,"outcome":outcome,"afterDigest":format!("{:x}",Sha256::digest(serde_json::to_vec(&restored.archive()?)?))}));
    }
    Ok(cases)
}
fn migration_signed(
    state: &State,
    origin: AccountId32,
    command: ProviderCommand,
    serial: u8,
    frozen: Option<&Value>,
) -> TestResult<SignedContractCallV1> {
    let call = UnsignedContractCallV1::new(
        state.shell.info.provider_instance_id,
        OperationId::from_bytes([serial; 16]),
        origin,
        state.shell.nonce(origin),
        UnixSeconds::new(10_000),
        command,
    )?;
    if let Some(frozen) = frozen {
        let stored: SignedContractCallV1 = serde_json::from_value(frozen["signed"].clone())?;
        assert_eq!(stored.call, call);
        return Ok(stored);
    }
    let seed = (1..=5)
        .find(|seed| signed_account(*seed).ok() == Some(origin))
        .ok_or("fixture key")?;
    let key = subxt_signer::sr25519::Keypair::from_secret_key([seed; 32])?;
    Ok(SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
        call,
    })
}
fn migration_inputs(serial: u8) -> execution::ExecutionInputs {
    execution::ExecutionInputs {
        version: 1,
        now: UnixSeconds::new(500),
        seed: [serial; 32],
    }
}
fn migration_prepare(
    state: &mut State,
    origin: AccountId32,
    command: ProviderCommand,
    serial: u8,
) -> TestResult<OperationReceipt> {
    let signed = migration_signed(state, origin, command, serial, None)?;
    let outcome = signed_apply(state, signed, migration_inputs(serial));
    let receipt: OperationReceipt = serde_json::from_value(outcome["receipt"].clone())?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    Ok(receipt)
}
fn migration_case(
    mut state: State,
    origin: AccountId32,
    command: ProviderCommand,
    serial: u8,
    label: &str,
    frozen: Option<&Value>,
) -> TestResult<Value> {
    // Native admission validates each source before the sandbox sees it.
    state = State::from_archive(state.archive()?, state.shell.info.root_account)?;
    let archive = serde_json::to_string(&state.archive()?)?;
    let signed = migration_signed(&state, origin, command, serial, frozen)?;
    let inputs = migration_inputs(serial);
    let outcome = signed_apply(&mut state, signed.clone(), inputs.clone());
    assert_eq!(outcome["receipt"]["outcome"]["type"], "Success");
    Ok(
        json!({"label":label,"archive":archive,"signed":signed,"inputs":inputs,"outcome":outcome,"afterDigest":format!("{:x}",Sha256::digest(serde_json::to_vec(&state.archive()?)?))}),
    )
}
fn migration_catalog_case(
    archives: &[RetainedArchive],
    frozen: Option<&Value>,
) -> TestResult<Value> {
    let root = signed_account(1)?;
    let first: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_str(&archives[0].archive)?;
    let mut catalog = State::from_archive(first.clone(), root)?;
    for offset in 0..14u8 {
        migration_prepare(
            &mut catalog,
            root,
            ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
                kind: CatalogKind::Skill,
                id: 1000 + u32::from(offset),
                name: format!("bulk-{offset}-{}", "x".repeat(22_000)),
            }),
            180 + offset,
        )?;
    }
    assert!(
        catalog
            .engine
            .skills
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .encode()
            .len()
            > MAX_SIGNABLE_BYTES + 1024
    );
    let case = migration_case(
        catalog,
        root,
        ProviderCommand::SetCompensationPolicy(CompensationPolicy {
            coordinator_percent: Percentage::new(20)?,
        }),
        195,
        "catalog above one ABI input",
        frozen,
    )?;
    Ok(case)
}
fn migration_project_case(source: &RetainedArchive, frozen: Option<&Value>) -> TestResult<Value> {
    let root = signed_account(1)?;
    let source: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_str(&source.archive)?;
    let mut project = State::from_archive(source, root)?;
    let client = signed_account(5)?;
    let id = migration_prepare(
        &mut project,
        client,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Large retained project".into(),
            description: "p".repeat(200_000),
        }),
        200,
    )?
    .created_entity_id
    .ok_or("project id")?;
    let coordinator = project
        .engine
        .projects
        .get(&id)
        .ok_or("project")?
        .coordinator;
    let template_project = project
        .engine
        .projects
        .values()
        .find(|p| !p.proposals.is_empty())
        .ok_or("proposal template")?;
    let template = &template_project.proposals[0];
    let generated_contracts::provider_core::ProposalContentReferenceV1::Signed(reference) =
        &template.content
    else {
        return Err("fixture metadata source".into());
    };
    let accepted = project
        .shell
        .receipts
        .get(&reference.operation_id)
        .ok_or("fixture creation")?;
    let call = UnsignedContractCallV1::decode_signable(&accepted.bytes)?;
    let record = generated_contracts::provider_core::ProposalMetadataRecordV1::from_command(
        call.provider_instance_id,
        call.origin,
        call.operation_id,
        &call.command,
    )
    .ok_or("fixture legacy definition")?;
    let mut definition = template.definition(&record)?;
    definition.description = "q".repeat(80_000);
    let proposal_id = migration_prepare(
        &mut project,
        coordinator,
        ProviderCommand::CreateProposal {
            project_id: id,
            proposal: definition,
        },
        201,
    )?
    .created_entity_id
    .ok_or("proposal id")?;
    assert!(project.engine.projects[&id].encode().len() < 16 * 1024);
    let case = migration_case(
        project,
        coordinator,
        ProviderCommand::SetProposalDelivery {
            project_id: id,
            proposal_id,
            request: ProposalDeliveryRequest {
                expected_revision: 0,
                delivery: DeliveryPreference::WithinOneMonth {},
            },
        },
        202,
        "proposal content above one storage cell",
        frozen,
    )?;
    Ok(case)
}
fn migration_grow_qualifications(state: &mut State, owner: AccountId32) -> TestResult {
    let root = signed_account(1)?;
    let mut qualifications = state.engine.workers[&owner].qualifications.clone();
    for offset in 0..512u32 {
        let id = 1000 + offset;
        let mut operation = [245; 16];
        operation[..4].copy_from_slice(&offset.to_le_bytes());
        let call = UnsignedContractCallV1::new(
            state.shell.info.provider_instance_id,
            OperationId::from_bytes(operation),
            root,
            state.shell.nonce(root),
            UnixSeconds::new(10_000),
            ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
                kind: CatalogKind::Skill,
                id,
                name: format!("capacity-{offset}"),
            }),
        )?;
        let key = subxt_signer::sr25519::Keypair::from_secret_key([1; 32])?;
        let signed = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
            call,
        };
        assert_eq!(
            signed_apply(state, signed, migration_inputs(245))["receipt"]["outcome"]["type"],
            "Success"
        );
        qualifications.skill_ids.push(id);
    }
    migration_prepare(
        state,
        owner,
        ProviderCommand::UpdateQualifications(UpdateQualificationsRequest { qualifications }),
        212,
    )?;
    // Creation names are now offchain; this retained qualification set fits a cell.
    let view = &state.engine.workers[&owner];
    assert!(
        (
            view.account,
            &view.content,
            &view.qualifications,
            view.mode,
            view.coordinator_eligible,
            view.calendar.calendar_id,
            &view.worker_score,
            &view.coordinator_score
        )
            .encode()
            .len()
            + 5
            < 16 * 1024
    );
    Ok(())
}
fn migration_worker_case(
    trace: &Value,
    source: &RetainedArchive,
    frozen: Option<&Value>,
) -> TestResult<Value> {
    let root = signed_account(1)?;
    let first: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_str(&source.archive)?;
    let mut worker = State::from_archive(first, root)?;
    let owner = signed_account(2)?;
    for step in trace["steps"].as_array().ok_or("steps")? {
        let mut signed: SignedContractCallV1 = serde_json::from_value(step["signed"].clone())?;
        if signed.call.origin == owner
            && let ProviderCommand::RegisterWorker(request) = &mut signed.call.command
        {
            request.display_name = "n".repeat(MAX_SIGNABLE_BYTES.checked_sub(200).ok_or("size")?);
            let key = subxt_signer::sr25519::Keypair::from_secret_key([2; 32])?;
            signed.signature =
                Sr25519Signature::from_bytes(key.sign(&signed.call.signable_bytes()?).0);
        }
        assert_eq!(
            signed_apply(
                &mut worker,
                signed,
                serde_json::from_value(step["inputs"].clone())?
            ),
            step["outcome"]
        );
    }
    migration_grow_qualifications(&mut worker, owner)?;
    let mut calendar = worker
        .engine
        .workers
        .get(&owner)
        .ok_or("worker")?
        .calendar
        .definition
        .clone();
    for year in 2027..2047 {
        for week in 1..=50 {
            calendar.overrides.push(WeekOverride {
                week: Week::new(year, week)?,
                capacity: calendar.default_weekly_minutes,
            });
        }
    }
    migration_prepare(
        &mut worker,
        owner,
        ProviderCommand::SetCalendar(calendar.clone()),
        210,
    )?;
    assert!(worker.engine.workers[&owner].encode().len() < 16 * 1024);
    let case = migration_case(
        worker,
        owner,
        ProviderCommand::SetCalendar(calendar),
        211,
        "large name plus later qualifications, calendar and reservations",
        frozen,
    )?;
    Ok(case)
}
fn migration_aggregate_cases(
    trace: &Value,
    archives: &[RetainedArchive],
    frozen: Option<&[Value]>,
) -> TestResult<Vec<Value>> {
    Ok(vec![
        migration_catalog_case(archives, frozen.and_then(|c| c.first()))?,
        migration_project_case(&archives[1], frozen.and_then(|c| c.get(1)))?,
        migration_worker_case(trace, &archives[0], frozen.and_then(|c| c.get(2)))?,
    ])
}
fn migration_planning_cases(
    archives: &[RetainedArchive],
    frozen: Option<&[Value]>,
) -> TestResult<Vec<Value>> {
    let mut source: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_str(&archives[1].archive)?;
    let project = source
        .checkpoint
        .projects
        .values_mut()
        .next()
        .ok_or("legacy project")?;
    let id = project.project_id;
    let client = project.client;
    project.planning.status = PlanningStatus::Delivered;
    project.planning.quote = Some(PlanningQuote {
        fee: Money::new(101),
        minutes: Minutes::new(30),
        window: WeekWindow::new(Week::new(2026, 1)?, Week::new(2026, 1)?)?,
    });
    project.planning.escrow = Money::new(101);
    project.planning.revision = 19;
    let balance = source
        .checkpoint
        .balances
        .get_mut(&client)
        .ok_or("client balance")?;
    *balance = balance.checked_sub(Money::new(101))?;
    let state = State::from_archive(source.clone(), source.checkpoint.info.root_account)?;
    let accepted = migration_case(
        state.clone(),
        client,
        ProviderCommand::AcceptPlanningDelivery {
            project_id: id,
            expected_revision: 19,
        },
        220,
        "legacy positive planning payout",
        frozen.and_then(|c| c.first()),
    )?;
    let frozen = migration_case(
        state,
        client,
        ProviderCommand::DisputePlanning {
            project_id: id,
            request: ReasonRequest {
                reason: "Retained paid planning dispute".into(),
            },
        },
        221,
        "legacy positive planning freeze",
        frozen.and_then(|c| c.get(1)),
    )?;
    Ok(vec![accepted, frozen])
}

fn migration_reference(frozen: Option<&MigrationReference>) -> TestResult<MigrationReference> {
    let trace: Value = serde_json::from_str(include_str!("fixtures/financial-signed.json"))?;
    let mut state = State::new(serde_json::from_value(trace["root"].clone())?)?;
    state.shell.info.provider_instance_id = serde_json::from_value(trace["generation"].clone())?;
    let mut reference = MigrationReference {
        archives: Vec::new(),
        digests: Vec::new(),
        priority_cases: Vec::new(),
        aggregate_cases: Vec::new(),
        planning_cases: Vec::new(),
    };
    let steps = trace["steps"].as_array().ok_or("steps")?;
    for before_step in 0..=steps.len() {
        if before_step == 53 {
            reference.priority_cases = retained_priority_cases(
                &state,
                &serde_json::from_value(steps[52]["signed"].clone())?,
                frozen.map(|reference| reference.priority_cases.as_slice()),
            )?;
        }
        if [0, 35, 70, 90, 113, 129].contains(&before_step) {
            reference.archives.push(RetainedArchive {
                before_step,
                archive: serde_json::to_string(&state.archive()?)?,
            });
        }
        if let Some(step) = steps.get(before_step) {
            assert_eq!(
                signed_apply(
                    &mut state,
                    serde_json::from_value(step["signed"].clone())?,
                    serde_json::from_value(step["inputs"].clone())?
                ),
                step["outcome"]
            );
            reference.digests.push(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&state.archive()?)?)
            ));
        }
    }
    reference.aggregate_cases = migration_aggregate_cases(
        &trace,
        &reference.archives,
        frozen.map(|r| r.aggregate_cases.as_slice()),
    )?;
    reference.planning_cases = migration_planning_cases(
        &reference.archives,
        frozen.map(|r| r.planning_cases.as_slice()),
    )?;
    Ok(reference)
}
#[test]
#[ignore = "Archive trace authoring requires an explicit output path"]
fn export_migration_reference() -> TestResult {
    std::fs::write(
        std::env::var("MIGRATION_REFERENCE_OUTPUT")?,
        serde_json::to_string_pretty(&migration_reference(None)?)?,
    )?;
    Ok(())
}
fn ordinary_qualification_capacity_reference(frozen: Option<&[Value]>) -> TestResult<Vec<Value>> {
    let reference: MigrationReference =
        serde_json::from_str(include_str!("fixtures/migration-signed.json"))?;
    let source = reference.aggregate_cases.get(2).ok_or("worker case")?;
    let archive = serde_json::from_str(source["archive"].as_str().ok_or("archive")?)?;
    let mut state = State::from_archive(archive, signed_account(1)?)?;
    let original: provider_execution::migration::CoreArchiveV3 =
        serde_json::from_str(&reference.archives.last().ok_or("snapshot")?.archive)?;
    let owner = signed_account(2)?;
    let large = state.engine.workers[&owner].qualifications.clone();
    let small = original.checkpoint.workers[&owner].qualifications.clone();
    let mut steps = Vec::new();
    for (index, qualifications) in [large.clone(), small, large].into_iter().enumerate() {
        let serial = 239u8.checked_add(u8::try_from(index)?).ok_or("serial")?;
        let signed = migration_signed(
            &state,
            owner,
            ProviderCommand::UpdateQualifications(UpdateQualificationsRequest { qualifications }),
            serial,
            frozen.and_then(|steps| steps.get(index)),
        )?;
        let inputs = migration_inputs(serial);
        let outcome = signed_apply(&mut state, signed.clone(), inputs.clone());
        assert_eq!(outcome["receipt"]["outcome"]["type"], "Success");
        steps.push(json!({"signed":signed, "inputs":inputs, "outcome":outcome,
            "afterDigest":format!("{:x}", Sha256::digest(serde_json::to_vec(&state.archive()?)?))}));
    }
    Ok(steps)
}
#[test]
#[ignore = "Capacity reference authoring requires an explicit output path"]
fn export_ordinary_qualification_capacity() -> TestResult {
    std::fs::write(
        std::env::var("QUALIFICATION_CAPACITY_REFERENCE")?,
        serde_json::to_string_pretty(&ordinary_qualification_capacity_reference(None)?)?,
    )?;
    Ok(())
}
#[test]
fn complete_ordinary_qualification_capacity_reference() -> TestResult {
    let expected: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/qualification-capacity-signed.json"))?;
    let actual = ordinary_qualification_capacity_reference(Some(&expected))?;
    assert_eq!(
        Sha256::digest(serde_json::to_vec(&actual)?),
        Sha256::digest(serde_json::to_vec(&expected)?)
    );
    Ok(())
}
#[test]
fn complete_retained_migration_reference() -> TestResult {
    let expected: MigrationReference =
        serde_json::from_str(include_str!("fixtures/migration-signed.json"))?;
    let actual = migration_reference(Some(&expected))?;
    // Keep failure output small; fixture archives include complete private history.
    assert_eq!(
        Sha256::digest(serde_json::to_vec(&actual)?),
        Sha256::digest(serde_json::to_vec(&expected)?)
    );
    Ok(())
}

#[test]
fn retained_r2a_creation_refs_survive_proposal_conversion() -> TestResult {
    let reference: MigrationReference =
        serde_json::from_str(include_str!("fixtures/proposal-predecessor.json"))?;
    let old: provider_execution::migration::CoreArchiveV2 = serde_json::from_str(
        &reference
            .archives
            .last()
            .ok_or("predecessor fixture")?
            .archive,
    )?;
    let bytes = serde_json::to_vec(&old)?;
    let (core, records) =
        crate::migration::from_predecessor(&bytes, old.checkpoint.info.root_account)?;
    assert_eq!(
        serde_json::to_vec(&core.checkpoint.workers)?,
        serde_json::to_vec(&old.checkpoint.workers)?
    );
    for (id, project) in &old.checkpoint.projects {
        assert_eq!(core.checkpoint.projects[id].content, project.content);
    }
    let records = records
        .into_iter()
        .map(|r| (crate::migration::source_hash(&r.hash_input()), r))
        .collect();
    let restored = crate::migration::to_predecessor(core, &records)?;
    assert_eq!(serde_json::to_vec(&restored)?, bytes);
    Ok(())
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "Replay the full signed scenario and check each commitment slot, rejection, replay and archive boundary."
)]
fn committed_evidence_preserves_receipts_money_replay_and_current_archives() -> TestResult {
    let trace: Value = serde_json::from_str(include_str!("fixtures/financial-signed.json"))?;
    let root = serde_json::from_value(trace["root"].clone())?;
    let mut state = State::new(root)?;
    state.shell.info.provider_instance_id = serde_json::from_value(trace["generation"].clone())?;
    let frozen: Value = serde_json::from_str(include_str!("fixtures/evidence-signed.json"))?;
    let authoring = std::env::var("EVIDENCE_FIXTURE_OUTPUT").ok();
    let mut counts = [0usize; 4];
    let mut replacements = Vec::new();
    for (index, step) in trace["steps"].as_array().ok_or("steps")?.iter().enumerate() {
        let mut signed: SignedContractCallV1 = serde_json::from_value(step["signed"].clone())?;
        let original = signed.clone();
        let inputs: execution::ExecutionInputs = serde_json::from_value(step["inputs"].clone())?;
        let evidence = EvidenceCommitmentV1 {
            record_id: EntityId::from_bytes(signed.call.operation_id.into_bytes()),
            hash: PayloadHash::from_bytes([91; 32]),
        };
        if execution::VerifiedCall::verify(original.clone()).is_ok() {
            signed.call.command = match signed.call.command {
                ProviderCommand::SubmitMilestoneDelivery {
                    project_id,
                    milestone_id,
                    request,
                } if request.deliverable.is_some() => ProviderCommand::SubmitMilestoneCommitted {
                    project_id,
                    milestone_id,
                    evidence,
                    worker_ratings: None,
                },
                ProviderCommand::RequestMilestoneCompletion {
                    project_id,
                    milestone_id,
                    request,
                } => ProviderCommand::SubmitMilestoneCommitted {
                    project_id,
                    milestone_id,
                    evidence,
                    worker_ratings: Some(request.worker_ratings),
                },
                ProviderCommand::RejectMilestoneCompletion {
                    project_id,
                    milestone_id,
                    submission_id,
                    ..
                } => ProviderCommand::RejectMilestoneCommitted {
                    project_id,
                    milestone_id,
                    submission_id,
                    evidence,
                },
                ProviderCommand::RejectMilestoneDelivery {
                    project_id,
                    milestone_id,
                    submission_id,
                    comment_id,
                } => ProviderCommand::RejectMilestoneCommitted {
                    project_id,
                    milestone_id,
                    submission_id,
                    evidence: EvidenceCommitmentV1 {
                        record_id: comment_id,
                        ..evidence
                    },
                },
                ProviderCommand::OpenDispute(request) => ProviderCommand::OpenDisputeCommitted {
                    project_id: request.project_id,
                    milestone_id: request.milestone_id,
                    rejected_submission_id: request.rejected_submission_id,
                    evidence,
                },
                ProviderCommand::OpenDisputeWithComment(request) => {
                    ProviderCommand::OpenDisputeCommitted {
                        project_id: request.project_id,
                        milestone_id: request.milestone_id,
                        rejected_submission_id: request.rejected_submission_id,
                        evidence: EvidenceCommitmentV1 {
                            record_id: request.comment_id,
                            ..evidence
                        },
                    }
                }
                ProviderCommand::RespondDispute {
                    project_id,
                    dispute_id,
                    ..
                } => ProviderCommand::RespondDisputeCommitted {
                    project_id,
                    dispute_id,
                    evidence,
                },
                other => other,
            };
        }
        let changed = signed.call.command != original.call.command;
        if changed {
            if authoring.is_some() {
                let seed = (1..=10)
                    .find(|seed| signed_account(*seed).ok() == Some(signed.call.origin))
                    .ok_or("fixture signing key")?;
                let key = subxt_signer::sr25519::Keypair::from_secret_key([seed; 32])?;
                signed.signature =
                    Sr25519Signature::from_bytes(key.sign(&signed.call.signable_bytes()?).0);
            } else {
                let expected = signed.call.clone();
                let replacement = frozen["replacements"]
                    .as_array()
                    .ok_or("replacements")?
                    .iter()
                    .find(|r| r["step"] == index)
                    .ok_or("frozen command")?;
                signed = serde_json::from_value(replacement["signed"].clone())?;
                assert_eq!(signed.call, expected, "frozen command {index}");
            }
            replacements.push(json!({"step":index,"signed":signed}));
        }
        let before = serde_json::to_value(&state.engine)?;
        let outcome = signed_apply(&mut state, signed.clone(), inputs.clone());
        assert_eq!(outcome, step["outcome"], "committed step {index}");
        if outcome["receipt"]["outcome"]["type"] == "Failed" || outcome.get("error").is_some() {
            assert_eq!(
                serde_json::to_value(&state.engine)?,
                before,
                "failed step {index}"
            );
        } else if changed {
            let command = &signed.call.command;
            let project = state
                .engine
                .projects
                .get(&command.project_id().ok_or("project")?)
                .ok_or("project")?;
            match command {
                ProviderCommand::SubmitMilestoneCommitted { evidence, .. } => {
                    counts[0] += 1;
                    let id: EntityId =
                        serde_json::from_value(outcome["receipt"]["createdEntityId"].clone())?;
                    let submission = project
                        .proposals
                        .iter()
                        .flat_map(|p| &p.milestones)
                        .flat_map(|m| &m.submissions)
                        .find(|s| s.submission_id == id)
                        .ok_or("submission")?;
                    assert_eq!(
                        submission.deliverable,
                        Some(EvidenceRecordReferenceV1::Committed(evidence.clone()))
                    );
                }
                ProviderCommand::RejectMilestoneCommitted {
                    submission_id,
                    evidence,
                    ..
                } => {
                    counts[1] += 1;
                    let submission = project
                        .proposals
                        .iter()
                        .flat_map(|p| &p.milestones)
                        .flat_map(|m| &m.submissions)
                        .find(|s| s.submission_id == *submission_id)
                        .ok_or("submission")?;
                    assert!(
                        matches!(&submission.review,SubmissionReview::Rejected{reason:EvidenceRecordReferenceV1::Committed(stored),..} if stored==evidence)
                    );
                }
                ProviderCommand::OpenDisputeCommitted { evidence, .. } => {
                    counts[2] += 1;
                    let id: EntityId =
                        serde_json::from_value(outcome["receipt"]["createdEntityId"].clone())?;
                    assert_eq!(
                        state.engine.disputes.get(&id).ok_or("case")?.evidence,
                        Some(EvidenceRecordReferenceV1::Committed(evidence.clone()))
                    );
                }
                ProviderCommand::RespondDisputeCommitted {
                    dispute_id,
                    evidence,
                    ..
                } => {
                    counts[3] += 1;
                    assert_eq!(
                        state
                            .engine
                            .disputes
                            .get(dispute_id)
                            .and_then(|d| d.response.as_ref())
                            .ok_or("response")?
                            .evidence,
                        EvidenceRecordReferenceV1::Committed(evidence.clone())
                    );
                }
                _ => return Err("unexpected commitment command".into()),
            }
            let encoded = state.encode()?;
            assert_eq!(
                signed_apply(&mut state, signed, inputs),
                outcome,
                "replay {index}"
            );
            assert_eq!(state.encode()?, encoded);
        }
        let encoded = state.encode()?;
        state = State::from_archive(state.archive()?, root)?;
        assert_eq!(state.encode()?, encoded, "archive {index}");
    }
    assert!(
        counts.iter().all(|count| *count > 0),
        "all operation kinds: {counts:?}"
    );
    assert_eq!(json!(projection(&state)?.1), trace["final"][1]);
    if let Some(path) = authoring {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({"version":1,"replacements":replacements}))?,
        )?;
    }
    Ok(())
}
