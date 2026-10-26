//! V5 keeps task identity/membership in core; content travels in the paired API export.
#![cfg(feature = "storage-sqlite")]
use generated_contracts::{
    AccountId32, ExecutionOutcome, OperationId, ProviderCommand, SignedContractCallV1,
    Sr25519Signature, UnixSeconds, UnsignedContractCallV1,
};
use mock_provider::{
    Provider,
    migration::{compact_tasks, restore_task_content},
};
use provider_execution::migration::{CoreArchiveV4, CoreArchiveV5};
use subxt_signer::sr25519::Keypair;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn archives() -> TestResult<Vec<CoreArchiveV4>> {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../src/domain/fixtures/migration-signed.json"))?;
    corpus["archives"]
        .as_array()
        .ok_or("archives")?
        .iter()
        .map(|entry| {
            Ok(serde_json::from_str(
                entry["archive"].as_str().ok_or("archive")?,
            )?)
        })
        .collect()
}
fn signer(account: AccountId32) -> TestResult<Keypair> {
    for seed in 1..=10 {
        let key = Keypair::from_secret_key([seed; 32])?;
        if AccountId32::from_bytes(key.public_key().0) == account {
            return Ok(key);
        }
    }
    Err("fixture signer missing".into())
}
async fn signed(
    provider: &Provider,
    origin: AccountId32,
    command: ProviderCommand,
    id: u8,
) -> TestResult<SignedContractCallV1> {
    let call = UnsignedContractCallV1::new(
        provider.info().await?.provider_instance_id,
        OperationId::from_bytes([id; 16]),
        origin,
        provider.nonce(origin).await?.nonce,
        UnixSeconds::new(2_000_000_000),
        command,
    )?;
    let signature = Sr25519Signature::from_bytes(signer(origin)?.sign(&call.signable_bytes()?).0);
    Ok(SignedContractCallV1 { call, signature })
}

#[test]
fn paired_conversion_preserves_all_retained_content_and_history() -> TestResult {
    for archive in archives()? {
        let exact = serde_json::to_value(&archive)?;
        let root = archive.checkpoint.info.root_account;
        let (core, tasks) = compact_tasks(root, archive)?;
        assert_eq!(core.version, 5);
        for project in core.checkpoint.projects.values() {
            for task in project
                .proposals
                .iter()
                .flat_map(|p| &p.milestones)
                .flat_map(|m| &m.task_storage.tasks)
            {
                let json = serde_json::to_value(task)?;
                assert_eq!(json.as_object().ok_or("task object")?.len(), 2);
                assert!(json.get("taskId").is_some());
                assert!(json.get("assignees").is_some());
            }
        }
        assert_eq!(
            serde_json::to_value(restore_task_content(root, core, tasks)?)?,
            exact
        );
    }
    Ok(())
}

#[test]
fn rollback_requires_actual_ids_membership_context_and_counter() -> TestResult {
    let archive = archives()?.remove(2);
    let root = archive.checkpoint.info.root_account;
    let (core, tasks) = compact_tasks(root, archive)?;
    let mut bad = tasks.clone();
    bad.next_task_id += 1;
    assert!(restore_task_content(root, core.clone(), bad).is_err());
    let mut bad = tasks.clone();
    bad.storages
        .iter_mut()
        .find_map(|r| r.storage.tasks.first_mut())
        .ok_or("task")?
        .task
        .assignees
        .clear();
    assert!(restore_task_content(root, core.clone(), bad).is_err());
    let mut bad = tasks.clone();
    bad.storages.pop();
    assert!(restore_task_content(root, core.clone(), bad).is_err());
    let mut current = tasks;
    let task = current
        .storages
        .iter_mut()
        .find_map(|r| r.storage.tasks.first_mut())
        .ok_or("task")?;
    task.task.description = "λ\0".repeat(20_000);
    task.updated_at = UnixSeconds::new(1_900_000_001);
    let restored = restore_task_content(root, core.clone(), current.clone())?;
    let (roundtrip, text) = CoreArchiveV5::from_v4(restored)?;
    assert_eq!(
        serde_json::to_value(roundtrip)?,
        serde_json::to_value(core)?
    );
    assert_eq!(text, current);
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep allocation, restart, membership and financial-isolation assertions in one signed scenario."
)]
async fn signed_compact_creation_records_actual_allocation_and_replays_after_reopen() -> TestResult
{
    let archive = archives()?.remove(1);
    let root = archive.checkpoint.info.root_account;
    let (core, _) = compact_tasks(root, archive)?;
    let project = core.checkpoint.projects.values().next().ok_or("project")?;
    let project_id = project.project_id;
    let origin = project.coordinator;
    let client = project.client;
    let storage = project.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    let next = core.checkpoint.next_task_id;
    let directory = std::env::temp_dir().join(format!(
        "task-core-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&directory)?;
    let url = format!("sqlite://{}", directory.join("tasks.db").display());
    let provider = Provider::sqlite_from_archive(&url, root, core).await?;
    let call = signed(
        &provider,
        origin,
        ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id: storage,
            assignees: vec![],
        },
        210,
    )
    .await?;
    let receipt = provider
        .execute(call.clone(), UnixSeconds::new(1_900_000_000))
        .await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(receipt.created_entity_id, Some(storage));
    let allocation = provider
        .task_allocation(receipt.operation_id)
        .await?
        .ok_or("allocation")?;
    assert_eq!(allocation.task_id, next);
    assert_eq!(allocation.receipt, receipt);
    assert_eq!(provider.task_state().await?.next_task_id, next + 1);
    drop(provider);
    let provider = Provider::sqlite(&url, root).await?;
    assert_eq!(
        provider
            .execute(call, UnixSeconds::new(2_000_000_001))
            .await?,
        receipt
    );
    assert_eq!(
        provider.task_allocation(receipt.operation_id).await?,
        Some(allocation)
    );
    assert_eq!(provider.task_state().await?.next_task_id, next + 1);
    let rejected = signed(
        &provider,
        client,
        ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id: storage,
            assignees: vec![],
        },
        211,
    )
    .await?;
    let rejection = provider
        .execute(rejected, UnixSeconds::new(1_900_000_000))
        .await?;
    assert_eq!(
        rejection.outcome,
        ExecutionOutcome::Failed("coordinator_required".into())
    );
    assert!(
        provider
            .task_allocation(rejection.operation_id)
            .await?
            .is_none()
    );
    assert_eq!(provider.task_state().await?.next_task_id, next + 1);
    let worker = provider
        .snapshot()
        .await?
        .workers
        .into_iter()
        .find(|w| w.account != origin && w.account != client)
        .ok_or("worker")?
        .account;
    let assignment = signed(
        &provider,
        origin,
        ProviderCommand::SetTaskAssigneesCore {
            project_id,
            task_storage_id: storage,
            task_id: next,
            assignees: vec![worker],
        },
        213,
    )
    .await?;
    let assigned = provider
        .execute(assignment, UnixSeconds::new(1_900_000_000))
        .await?;
    assert_eq!(assigned.outcome, ExecutionOutcome::Success);
    let event = provider
        .events(assigned.first_event_cursor.ok_or("cursor")? - 1, 1)
        .await?
        .events
        .remove(0);
    assert!(event.recipients.contains(&worker));
    assert!(
        !provider
            .events(receipt.first_event_cursor.ok_or("cursor")? - 1, 1)
            .await?
            .events[0]
            .recipients
            .contains(&worker)
    );
    let proposal = provider
        .snapshot()
        .await?
        .projects
        .into_iter()
        .find(|p| p.project_id == project_id)
        .ok_or("project")?
        .proposals
        .remove(0);
    let withdraw = signed(
        &provider,
        origin,
        ProviderCommand::WithdrawProposal {
            project_id,
            proposal_id: proposal.proposal_id,
            expected_revision: proposal.revision,
        },
        214,
    )
    .await?;
    let withdrawn = provider
        .execute(withdraw, UnixSeconds::new(1_900_000_000))
        .await?;
    assert_eq!(withdrawn.outcome, ExecutionOutcome::Success);
    assert!(
        provider
            .events(withdrawn.first_event_cursor.ok_or("cursor")? - 1, 1)
            .await?
            .events[0]
            .recipients
            .contains(&worker)
    );
    let legacy = signed(
        &provider,
        origin,
        ProviderCommand::UpdateTaskProgress {
            project_id,
            task_storage_id: storage,
            task_id: next,
            progress: generated_contracts::TaskProgressRequest {
                status: generated_contracts::TaskStatus::InProgress,
                logged_minutes: generated_contracts::Minutes::ZERO,
            },
        },
        212,
    )
    .await?;
    let before = serde_json::to_value(provider.archive().await?)?;
    assert!(
        provider
            .execute(legacy, UnixSeconds::new(1_900_000_000))
            .await
            .err()
            .ok_or("fresh legacy rejected")?
            .is_infrastructure()
    );
    assert_eq!(serde_json::to_value(provider.archive().await?)?, before);
    drop(provider);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

/// This is a partial retained-history fixture, not an importable provider archive.
/// Full V4 import/reopen is exercised separately against actual retained archives.
#[test]
fn original_edit_task_bytes_replay_both_success_and_rejection_without_fresh_execution() -> TestResult
{
    use provider_execution::{Admission, ExecutionInputs, RecordedCall, Shell, VerifiedCall};
    let trace: serde_json::Value =
        serde_json::from_str(include_str!("../src/domain/fixtures/project-signed.json"))?;
    for (index, outcome) in [
        (20, ExecutionOutcome::Failed("coordinator_required".into())),
        (21, ExecutionOutcome::Success),
    ] {
        let step = &trace["steps"][index];
        let signed: SignedContractCallV1 = serde_json::from_value(step["signed"].clone())?;
        assert!(matches!(
            signed.call.command,
            ProviderCommand::EditTask { .. }
        ));
        let receipt: generated_contracts::OperationReceipt =
            serde_json::from_value(step["outcome"]["receipt"].clone())?;
        assert_eq!(receipt.outcome, outcome);
        let mut shell = Shell::new(serde_json::from_value(trace["root"].clone())?)?;
        shell.info.provider_instance_id = signed.call.provider_instance_id;
        shell.nonces.insert(
            signed.call.origin,
            signed.call.nonce.checked_add(1).ok_or("nonce")?,
        );
        shell.receipts.insert(
            signed.call.operation_id,
            RecordedCall {
                bytes: signed.call.signable_bytes()?,
                inputs: Some(serde_json::from_value(step["inputs"].clone())?),
                receipt: receipt.clone(),
                allocated_task_id: None,
            },
        );
        let before = serde_json::to_vec(&shell)?;
        let observed = shell.prepare_with_inputs(
            VerifiedCall::verify(signed)?,
            ExecutionInputs {
                version: 99,
                now: UnixSeconds::new(u64::MAX),
                seed: [0; 32],
            },
        )?;
        let Admission::Replay(observed) = observed else {
            return Err("retained EditTask was admitted as fresh work".into());
        };
        assert_eq!(observed, receipt);
        assert_eq!(serde_json::to_vec(&shell)?, before);
    }
    Ok(())
}
