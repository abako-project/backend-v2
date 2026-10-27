//! Small current-state scenarios: independent outcomes, ordinary commands and durable restart.
use contract_provider::{
    EXECUTION_GAS,
    worker::{Client, Configuration, Worker},
};
use protocol::*;
use provider::ProviderApi;
use sp_core::{Pair, sr25519};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: UnixSeconds = UnixSeconds::new(100);
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "contract-provider-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!path.exists());
        Self(path)
    }
    async fn pool(&self) -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&self.0))
            .await
            .unwrap()
    }
    async fn open(&self) -> Worker {
        Worker::open(self.0.clone(), configuration()).await.unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.0.exists() {
            std::fs::remove_file(&self.0).unwrap();
        }
    }
}
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn configuration() -> Configuration {
    Configuration {
        root: account(1),
        mock_funding: true,
        gas_limit: EXECUTION_GAS,
    }
}
async fn sign(
    client: &Client,
    seed: u8,
    operation: u8,
    command: ProviderCommand,
) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        client.info().await.unwrap().provider_instance_id,
        OperationId::from_bytes([operation; 16]),
        account(seed),
        client.nonce(account(seed)).await.unwrap().nonce,
        UnixSeconds::new(200),
        command,
    )
    .unwrap();
    let signature = Sr25519Signature::from_bytes(
        sr25519::Pair::from_seed(&[seed; 32])
            .sign(&call.signable_bytes().unwrap())
            .0,
    );
    SignedContractCallV1 { call, signature }
}
fn fund(amount: u64) -> ProviderCommand {
    ProviderCommand::FundAccount(FundAccountRequest {
        account: account(2),
        amount: Money::new(amount),
    })
}
async fn balance(client: &Client) -> Money {
    client
        .snapshot()
        .await
        .unwrap()
        .balances
        .into_iter()
        .find(|b| b.account == account(2))
        .map_or(Money::ZERO, |b| b.available)
}

#[tokio::test]
async fn fresh_genesis_execution_replay_permissions_and_restart() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let before = client.snapshot().await.unwrap();
    assert!(before.projects.is_empty());
    assert!(before.workers.is_empty());
    assert!(
        before.balances.is_empty(),
        "technical endowments are not visible money"
    );
    assert_eq!(
        before
            .catalog
            .roles
            .iter()
            .filter(|role| role.fixed)
            .map(|role| role.id)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(client.task_state().await.unwrap().next_task_id, 1);
    assert!(!client.task_state().await.unwrap().has_task_history);
    let call = sign(&client, 1, 1, fund(37)).await;
    let receipt = ProviderApi::execute(&client, call.clone(), NOW)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(balance(&client).await, Money::new(37));
    assert_eq!(
        ProviderApi::execute(&client, call.clone(), NOW)
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(client.nonce(account(1)).await.unwrap().nonce, 1);
    assert_eq!(client.events(0, 10).await.unwrap().events.len(), 1);
    let denied = sign(&client, 2, 2, fund(100)).await;
    let denied = ProviderApi::execute(&client, denied, NOW).await.unwrap();
    assert!(matches!(denied.outcome, ExecutionOutcome::Failed(_)));
    assert_eq!(balance(&client).await, Money::new(37));
    assert_eq!(client.nonce(account(2)).await.unwrap().nonce, 1);
    assert_eq!(client.events(0, 10).await.unwrap().events.len(), 1);
    let mut invalid = sign(&client, 1, 3, fund(3)).await;
    invalid.signature = Sr25519Signature::from_bytes([0; 64]);
    assert!(ProviderApi::execute(&client, invalid, NOW).await.is_err());
    assert_eq!(client.nonce(account(1)).await.unwrap().nonce, 1);
    let expected = client.snapshot().await.unwrap();
    worker.shutdown().await.unwrap();
    let worker = database.open().await;
    let reopened = worker.client();
    assert_eq!(reopened.snapshot().await.unwrap(), expected);
    assert_eq!(
        reopened.receipt(receipt.operation_id).await.unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        ProviderApi::execute(&reopened, call, NOW).await.unwrap(),
        receipt
    );
    assert_eq!(balance(&reopened).await, Money::new(37));
    worker.shutdown().await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&database.0).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn failed_commit_rolls_back_money_nonce_receipt_and_events() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let before = client.snapshot().await.unwrap();
    let call = sign(&client, 1, 1, fund(37)).await;
    let pool = database.pool().await;
    sqlx::query("CREATE TRIGGER fail_commit BEFORE UPDATE ON provider_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
    assert!(
        ProviderApi::execute(&client, call.clone(), NOW)
            .await
            .is_err()
    );
    assert_eq!(client.snapshot().await.unwrap(), before);
    assert_eq!(client.nonce(account(1)).await.unwrap().nonce, 0);
    assert!(
        client
            .receipt(call.call.operation_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(client.events(0, 10).await.unwrap().events.is_empty());
    sqlx::query("DROP TRIGGER fail_commit")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(
        ProviderApi::execute(&client, call, NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Success
    );
    assert_eq!(balance(&client).await, Money::new(37));
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn bank_reads_enforce_owner_or_system_and_credits_once() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let amount = KvnAmount::new(Money::new(37)).unwrap();
    let create = sign(
        &client,
        2,
        1,
        ProviderCommand::CreateDeposit(CreateDepositRequest { amount }),
    )
    .await;
    let deposit = ProviderApi::execute(&client, create, NOW)
        .await
        .unwrap()
        .created_entity_id
        .unwrap();
    assert_eq!(balance(&client).await, Money::ZERO);
    assert_eq!(
        client
            .bramp_deposit(account(2), deposit)
            .await
            .unwrap()
            .status,
        DepositStatus::Pending
    );
    assert!(client.bramp_deposit(account(1), deposit).await.is_ok());
    assert_eq!(
        client
            .bramp_deposit(account(3), deposit)
            .await
            .unwrap_err()
            .code(),
        "deposit_owner_or_system_required"
    );
    let confirm = sign(
        &client,
        1,
        2,
        ProviderCommand::ConfirmDeposit {
            deposit_id: deposit,
        },
    )
    .await;
    assert_eq!(
        ProviderApi::execute(&client, confirm.clone(), NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Success
    );
    ProviderApi::execute(&client, confirm, NOW).await.unwrap();
    assert_eq!(balance(&client).await, Money::new(37));
    let withdraw = sign(
        &client,
        2,
        3,
        ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest { amount }),
    )
    .await;
    let withdrawal = ProviderApi::execute(&client, withdraw, NOW)
        .await
        .unwrap()
        .created_entity_id
        .unwrap();
    assert_eq!(balance(&client).await, Money::ZERO);
    assert!(
        client
            .bramp_withdrawal(account(2), withdrawal)
            .await
            .is_ok()
    );
    assert!(
        client
            .bramp_withdrawal(account(1), withdrawal)
            .await
            .is_ok()
    );
    assert_eq!(
        client
            .bramp_withdrawal(account(3), withdrawal)
            .await
            .unwrap_err()
            .code(),
        "withdrawal_owner_or_system_required"
    );
    let cancel = sign(
        &client,
        2,
        4,
        ProviderCommand::CancelWithdrawal {
            withdrawal_id: withdrawal,
        },
    )
    .await;
    assert_eq!(
        ProviderApi::execute(&client, cancel, NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Success
    );
    assert_eq!(balance(&client).await, Money::new(37));
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn exhausted_execution_preserves_admission_and_existing_empty_store_is_rejected() {
    let database = Database::new();
    let mut settings = configuration();
    settings.gas_limit = frame_support::weights::Weight::from_parts(1, 1);
    let worker = Worker::open(database.0.clone(), settings).await.unwrap();
    let client = worker.client();
    let call = sign(&client, 1, 1, fund(37)).await;
    assert!(matches!(
        client
            .execute_signed(call.clone(), NOW)
            .unwrap()
            .result()
            .await,
        Err(contract_provider::worker::Error::ExecutionLimit(_))
    ));
    assert_eq!(client.nonce(account(1)).await.unwrap().nonce, 0);
    assert!(
        client
            .receipt(call.call.operation_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(client.events(0, 10).await.unwrap().events.is_empty());
    worker.shutdown().await.unwrap();
    assert!(
        Worker::open(database.0.clone(), configuration())
            .await
            .is_err(),
        "incompatible configuration must fail"
    );
    std::fs::write(&database.0, []).unwrap();
    assert!(
        Worker::open(database.0.clone(), configuration())
            .await
            .is_err(),
        "an existing empty/corrupt store must not be reseeded"
    );
}

fn content(operation: u8) -> protocol::provider_core::MetadataCommitmentV1 {
    protocol::provider_core::MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([operation; 16]),
        hash: PayloadHash::from_bytes([operation; 32]),
    }
}
async fn apply(
    client: &Client,
    seed: u8,
    operation: u8,
    command: ProviderCommand,
) -> OperationReceipt {
    let receipt = ProviderApi::execute(client, sign(client, seed, operation, command).await, NOW)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    receipt
}
#[tokio::test]
async fn task_identity_membership_private_allocation_and_event_audience_use_current_contracts() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let (project_id, proposal_id, task_storage_id) = project(&client).await;
    let call = sign(
        &client,
        2,
        12,
        ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id,
            assignees: vec![account(3)],
        },
    )
    .await;
    let receipt = ProviderApi::execute(&client, call.clone(), NOW)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    let allocation = client
        .task_allocation(receipt.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(allocation.receipt, receipt);
    assert_eq!(allocation.task_id, 1);
    assert_eq!(allocation.task_storage_id, task_storage_id);
    assert_eq!(
        ProviderApi::execute(&client, call, NOW).await.unwrap(),
        receipt
    );
    assert_eq!(client.task_state().await.unwrap().next_task_id, 2);
    assert!(client.task_state().await.unwrap().has_task_history);
    apply(
        &client,
        2,
        13,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
    )
    .await;
    let events = client.events(0, 100).await.unwrap().events;
    let submitted = events.last().unwrap();
    assert_eq!(submitted.kind, DomainEventKind::ProposalSubmitted);
    assert!(
        submitted.recipients.contains(&account(3)),
        "task membership is observed at execution"
    );
    apply(
        &client,
        2,
        14,
        ProviderCommand::SetTaskAssigneesCore {
            project_id,
            task_storage_id,
            task_id: 1,
            assignees: vec![],
        },
    )
    .await;
    let revision = client.snapshot().await.unwrap().projects[0].proposals[0].revision;
    apply(
        &client,
        2,
        15,
        ProviderCommand::WithdrawProposal {
            project_id,
            proposal_id,
            expected_revision: revision,
        },
    )
    .await;
    let events = client.events(0, 100).await.unwrap().events;
    assert!(!events.last().unwrap().recipients.contains(&account(3)));
    let revision = client.snapshot().await.unwrap().projects[0].proposals[0].revision;
    apply(
        &client,
        2,
        16,
        ProviderCommand::DeleteTask {
            project_id,
            task_storage_id,
            task_id: 1,
            expected_revision: revision,
        },
    )
    .await;
    worker.shutdown().await.unwrap();
    let worker = database.open().await;
    let client = worker.client();
    assert_eq!(
        client.task_state().await.unwrap().next_task_id,
        2,
        "deletion never reuses a task ID"
    );
    assert!(
        client.task_state().await.unwrap().storages[0]
            .storage
            .tasks
            .is_empty()
    );
    assert_eq!(
        client.task_allocation(receipt.operation_id).await.unwrap(),
        Some(allocation)
    );
    worker.shutdown().await.unwrap();
}

async fn project(client: &Client) -> (EntityId, EntityId, EntityId) {
    use protocol::provider_core::{MilestoneTerms, ProposalTerms, RegisterWorkerCoreRequest};
    for seed in [2, 3] {
        apply(
            client,
            seed,
            seed,
            ProviderCommand::RegisterWorkerCore(RegisterWorkerCoreRequest {
                content: content(seed),
                qualifications: Qualifications {
                    role_ids: vec![1, 3],
                    skill_ids: vec![1],
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(5000),
                    overrides: vec![],
                },
            }),
        )
        .await;
    }
    apply(
        client,
        1,
        4,
        ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
            account: account(2),
        }),
    )
    .await;
    apply(
        client,
        2,
        5,
        ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
            mode: WorkerMode::Coordinator,
        }),
    )
    .await;
    let request = apply(
        client,
        3,
        6,
        ProviderCommand::CreateSkillRequestCore {
            role_ids: vec![3],
            content: content(6),
        },
    )
    .await;
    assert_eq!(
        client.skill_requests(account(3)).await.unwrap()[0].request_id,
        request.created_entity_id.unwrap()
    );
    assert!(client.skill_requests(account(2)).await.unwrap().is_empty());
    assert_eq!(client.skill_requests(account(1)).await.unwrap().len(), 1);
    apply(
        client,
        1,
        7,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: account(4),
            amount: Money::new(1000),
        }),
    )
    .await;
    let project_id = apply(client, 4, 8, ProviderCommand::CreateProjectCore(content(8)))
        .await
        .created_entity_id
        .unwrap();
    let window = WeekWindow::new(Week::new(2026, 1).unwrap(), Week::new(2026, 2).unwrap()).unwrap();
    let proposal_id = apply(
        client,
        2,
        11,
        ProviderCommand::CreateProposalCore {
            project_id,
            content: content(11),
            proposal: ProposalTerms {
                milestones: vec![MilestoneTerms {
                    key: 1,
                    window,
                    coordinator_fee: Money::ZERO,
                    coordinator_minutes: Minutes::new(60),
                    requirements: vec![RequirementDefinition {
                        key: 1,
                        role_id: 3,
                        skill_ids: vec![1],
                        minutes: Minutes::new(60),
                        budget: Money::new(100),
                    }],
                }],
            },
        },
    )
    .await
    .created_entity_id
    .unwrap();
    let snapshot = client.snapshot().await.unwrap();
    let task_storage_id = snapshot.projects[0].proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    (project_id, proposal_id, task_storage_id)
}

async fn ready(client: &Client) -> (EntityId, EntityId, EntityId) {
    let (project_id, proposal_id, _) = project(client).await;
    let expected_revision = client.snapshot().await.unwrap().projects[0].proposals[0].revision;
    apply(
        client,
        2,
        20,
        ProviderCommand::SetProposalDelivery {
            project_id,
            proposal_id,
            request: ProposalDeliveryRequest {
                expected_revision,
                delivery: DeliveryPreference::WithinOneMonth {},
            },
        },
    )
    .await;
    apply(
        client,
        2,
        21,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
    )
    .await;
    let revision = client.snapshot().await.unwrap().projects[0]
        .planning
        .revision;
    apply(
        client,
        4,
        22,
        ProviderCommand::AcceptPlanningDelivery {
            project_id,
            expected_revision: revision,
        },
    )
    .await;
    let snapshot = client.snapshot().await.unwrap();
    let milestone = snapshot.projects[0].proposals[0].milestones[0].milestone_id;
    (project_id, proposal_id, milestone)
}
async fn approval(
    client: &Client,
    project_id: EntityId,
    proposal_id: EntityId,
) -> SignedContractCallV1 {
    let expected_revision = client.snapshot().await.unwrap().projects[0].proposals[0].revision;
    sign(
        client,
        4,
        23,
        ProviderCommand::ApproveExecution {
            project_id,
            proposal_id,
            expected_revision,
        },
    )
    .await
}
fn evidence(id: u8) -> EvidenceCommitmentV1 {
    EvidenceCommitmentV1 {
        record_id: EntityId::from_bytes([id; 16]),
        hash: PayloadHash::from_bytes([id; 32]),
    }
}
async fn delivery(client: &Client, project_id: EntityId, milestone_id: EntityId) -> EntityId {
    apply(
        client,
        2,
        24,
        ProviderCommand::SubmitMilestoneCommitted {
            project_id,
            milestone_id,
            evidence: evidence(24),
            worker_ratings: None,
        },
    )
    .await;
    client.snapshot().await.unwrap().projects[0].proposals[0].milestones[0].submissions[0]
        .submission_id
}
fn funds(snapshot: &protocol::provider_core::CoreSnapshot, owner: u8) -> Money {
    snapshot
        .balances
        .iter()
        .find(|balance| balance.account == account(owner))
        .map_or(Money::ZERO, |balance| balance.available)
}
#[tokio::test]
async fn project_approval_commit_failure_reservation_settlement_and_restart() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let (project_id, proposal_id, milestone_id) = ready(&client).await;
    let call = approval(&client, project_id, proposal_id).await;
    let before = client.snapshot().await.unwrap();
    let events = client.events(0, 100).await.unwrap();
    let nonce = client.nonce(account(4)).await.unwrap();
    let pool = database.pool().await;
    sqlx::query("CREATE TRIGGER fail_commit BEFORE UPDATE ON provider_state BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&pool).await.unwrap();
    assert!(
        ProviderApi::execute(&client, call.clone(), NOW)
            .await
            .is_err()
    );
    assert_eq!(
        client.snapshot().await.unwrap(),
        before,
        "Project, escrow, free balances and Calendar reservations all roll back"
    );
    assert_eq!(client.events(0, 100).await.unwrap(), events);
    assert_eq!(client.nonce(account(4)).await.unwrap(), nonce);
    assert!(
        client
            .receipt(call.call.operation_id)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("DROP TRIGGER fail_commit")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(
        ProviderApi::execute(&client, call, NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Success
    );
    let approved = client.snapshot().await.unwrap();
    assert_eq!(funds(&approved, 4), Money::new(900));
    assert_eq!(approved.projects[0].execution_escrow, Money::new(100));
    for owner in [2, 3] {
        let worker = approved
            .workers
            .iter()
            .find(|worker| worker.account == account(owner))
            .unwrap();
        assert_eq!(
            worker
                .calendar
                .reservations
                .iter()
                .map(|r| u64::from(r.minutes.get()))
                .sum::<u64>(),
            60
        );
    }
    let submission_id = delivery(&client, project_id, milestone_id).await;
    apply(
        &client,
        4,
        25,
        ProviderCommand::AcceptMilestoneDelivery {
            project_id,
            milestone_id,
            submission_id,
        },
    )
    .await;
    let settled = client.snapshot().await.unwrap();
    assert!(settled.projects[0].completed);
    assert_eq!(settled.projects[0].execution_escrow, Money::ZERO);
    assert_eq!(funds(&settled, 4), Money::new(900));
    assert_eq!(funds(&settled, 2), Money::new(10));
    assert_eq!(funds(&settled, 3), Money::new(90));
    for (author, operation, recipients) in [(4, 26, vec![2]), (2, 27, vec![4, 3])] {
        apply(
            &client,
            author,
            operation,
            ProviderCommand::EvaluateProject {
                project_id,
                request: EvaluateProjectRequest {
                    ratings: recipients
                        .into_iter()
                        .map(|owner| AccountRating {
                            account: account(owner),
                            score: Score::new(5).unwrap(),
                        })
                        .collect(),
                },
            },
        )
        .await;
    }
    let evaluated = client.snapshot().await.unwrap();
    for owner in [2, 3] {
        let worker = evaluated
            .workers
            .iter()
            .find(|worker| worker.account == account(owner))
            .unwrap();
        let score = if owner == 2 {
            &worker.coordinator_score
        } else {
            &worker.worker_score
        };
        assert_eq!(score.rated_minutes, 60);
        assert_eq!(score.weighted_score_sum, 30_000);
    }
    worker.shutdown().await.unwrap();
    let worker = database.open().await;
    assert_eq!(worker.client().snapshot().await.unwrap(), evaluated);
    worker.shutdown().await.unwrap();
}
#[tokio::test]
async fn committed_rejection_dispute_freeze_and_private_reader_survive_restart() {
    let database = Database::new();
    let worker = database.open().await;
    let client = worker.client();
    let (project_id, proposal_id, milestone_id) = ready(&client).await;
    let call = approval(&client, project_id, proposal_id).await;
    assert_eq!(
        ProviderApi::execute(&client, call, NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Success
    );
    let submission_id = delivery(&client, project_id, milestone_id).await;
    apply(
        &client,
        4,
        25,
        ProviderCommand::RejectMilestoneCommitted {
            project_id,
            milestone_id,
            submission_id,
            evidence: evidence(25),
        },
    )
    .await;
    let receipt = apply(
        &client,
        2,
        26,
        ProviderCommand::OpenDisputeCommitted {
            project_id,
            milestone_id,
            rejected_submission_id: submission_id,
            evidence: evidence(26),
        },
    )
    .await;
    let id = receipt.created_entity_id.unwrap();
    let view = client.dispute(id).await.unwrap();
    assert_eq!(view.dispute.project_id, project_id);
    assert_eq!(view.dispute.rejected_submission_id, submission_id);
    assert!(view.milestone.frozen);
    assert_eq!(
        view.dispute.evidence,
        Some(EvidenceRecordReferenceV1::Committed(evidence(26)))
    );
    assert_eq!(
        client.snapshot().await.unwrap().projects[0].execution_escrow,
        Money::new(100)
    );
    let forbidden = sign(
        &client,
        4,
        27,
        ProviderCommand::AcceptMilestoneDelivery {
            project_id,
            milestone_id,
            submission_id,
        },
    )
    .await;
    assert!(matches!(
        ProviderApi::execute(&client, forbidden, NOW)
            .await
            .unwrap()
            .outcome,
        ExecutionOutcome::Failed(_)
    ));
    worker.shutdown().await.unwrap();
    let worker = database.open().await;
    assert_eq!(worker.client().dispute(id).await.unwrap(), view);
    worker.shutdown().await.unwrap();
}
