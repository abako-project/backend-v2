//! Compact task execution must agree across all three actual authority modes.
use generated_contracts::*;
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::{ReferenceState, migration::compact_tasks};
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV4};
use sp_core::{Pair, sr25519};

#[tokio::test]
async fn compact_task_allocation_and_execution_time_membership_match_in_all_authorities() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    let previous: CoreArchiveV4 =
        serde_json::from_str(corpus["archives"][2]["archive"].as_str().unwrap()).unwrap();
    let root = previous.checkpoint.info.root_account;
    let (initial, _) = compact_tasks(root, previous).unwrap();
    let project = initial
        .checkpoint
        .projects
        .values()
        .find(|p| p.proposals[0].status == ProposalStatus::PendingApproval)
        .unwrap();
    let origin = project.coordinator;
    let project_id = project.project_id;
    let proposal_id = project.proposals[0].proposal_id;
    let revision = project.proposals[0].revision;
    let storage = project.proposals[0].milestones[0]
        .task_storage
        .task_storage_id;
    let allocated = initial.checkpoint.next_task_id;
    let member = initial
        .checkpoint
        .workers
        .keys()
        .find(|account| **account != origin && **account != project.client)
        .copied()
        .unwrap();
    let key = (1..=10)
        .map(|seed| sr25519::Pair::from_seed(&[seed; 32]))
        .find(|key| AccountId32::from_bytes(key.public().0) == origin)
        .unwrap();
    let configuration = WholeConfiguration {
        root,
        generation: initial.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    };
    for (index, authority) in [Authority::Rust, Authority::Shadow, Authority::Sandbox]
        .into_iter()
        .enumerate()
    {
        let path = std::env::temp_dir().join(format!(
            "task-core-sdk-{}-{index}.sqlite",
            std::process::id()
        ));
        assert!(!path.exists());
        let worker = Worker::import_whole(
            path.clone(),
            initial.clone(),
            configuration.clone(),
            authority,
        )
        .await
        .unwrap();
        let mut reference = ReferenceState::from_archive(root, initial.clone()).unwrap();
        let commands = [
            ProviderCommand::CreateTaskCore {
                project_id,
                task_storage_id: storage,
                assignees: vec![],
            },
            ProviderCommand::SetTaskAssigneesCore {
                project_id,
                task_storage_id: storage,
                task_id: allocated,
                assignees: vec![member],
            },
            ProviderCommand::WithdrawProposal {
                project_id,
                proposal_id,
                expected_revision: revision,
            },
        ];
        let mut retained = None;
        for (sequence, command) in commands.into_iter().enumerate() {
            let call = UnsignedContractCallV1::new(
                configuration.generation,
                OperationId::from_bytes([u8::try_from(sequence).unwrap() + 220; 16]),
                origin,
                reference.nonce(origin).nonce,
                UnixSeconds::new(1_900_000_000),
                command,
            )
            .unwrap();
            let signature =
                Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0);
            let signed = SignedContractCallV1 { call, signature };
            let inputs = ExecutionInputs {
                version: 1,
                now: UnixSeconds::new(1_800_000_000),
                seed: [239; 32],
            };
            let (next, expected) = reference
                .execute(
                    VerifiedCall::verify(signed.clone()).unwrap(),
                    inputs.clone(),
                )
                .unwrap();
            assert_eq!(expected.outcome, ExecutionOutcome::Success);
            reference = next;
            let receipt = worker
                .client()
                .execute_directory_with_inputs(signed.clone(), inputs)
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(receipt, expected);
            let archive = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(&archive).unwrap(),
                serde_json::to_value(reference.archive().unwrap()).unwrap()
            );
            if sequence == 0 {
                let allocation = worker
                    .client()
                    .read_reference(move |state| state.task_allocation(receipt.operation_id))
                    .unwrap()
                    .result()
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(allocation.task_id, allocated);
                assert_eq!(allocation.receipt.created_entity_id, Some(storage));
                retained = Some((signed, receipt));
            } else {
                assert!(
                    archive
                        .checkpoint
                        .events
                        .last()
                        .unwrap()
                        .recipients
                        .contains(&member)
                );
            }
        }
        worker.shutdown().await.unwrap();
        let worker = Worker::reopen_whole(path.clone(), configuration.clone(), authority)
            .await
            .unwrap();
        let (signed, receipt) = retained.unwrap();
        let replay = worker
            .client()
            .execute_directory_with_inputs(
                signed,
                ExecutionInputs {
                    version: 1,
                    now: UnixSeconds::new(2_000_000_000),
                    seed: [1; 32],
                },
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(replay, receipt);
        let archive = worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(archive).unwrap(),
            serde_json::to_value(reference.archive().unwrap()).unwrap()
        );
        worker.shutdown().await.unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn retained_v4_task_calls_replay_after_import_but_fresh_content_commands_do_not_execute() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    let previous: CoreArchiveV4 = serde_json::from_str(
        corpus["archives"].as_array().unwrap().last().unwrap()["archive"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let original = serde_json::to_vec(&previous).unwrap();
    let root = previous.checkpoint.info.root_account;
    let (initial, tasks) = compact_tasks(root, previous).unwrap();
    assert_eq!(
        serde_json::to_vec(&initial.clone().into_v4(tasks).unwrap()).unwrap(),
        original
    );
    let trace: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
    ))
    .unwrap();
    let retained: Vec<SignedContractCallV1> = trace["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| serde_json::from_value(s["signed"].clone()).unwrap())
        .filter(|s: &SignedContractCallV1| {
            initial
                .checkpoint
                .receipts
                .contains_key(&s.call.operation_id)
        })
        .collect();
    let create = retained
        .iter()
        .find(|s| matches!(s.call.command, ProviderCommand::CreateTask { .. }))
        .unwrap();
    let progress = retained
        .iter()
        .find(|s| matches!(s.call.command, ProviderCommand::UpdateTaskProgress { .. }))
        .unwrap();
    let ProviderCommand::CreateTask {
        project_id,
        task_storage_id,
        task,
    } = create.call.command.clone()
    else {
        unreachable!()
    };
    let commands = [
        create.call.command.clone(),
        ProviderCommand::EditTask {
            project_id,
            task_storage_id,
            task_id: 1,
            task,
        },
        progress.call.command.clone(),
    ];
    let origin = create.call.origin;
    let key = (1..=10)
        .map(|seed| sr25519::Pair::from_seed(&[seed; 32]))
        .find(|k| AccountId32::from_bytes(k.public().0) == origin)
        .unwrap();
    let config = WholeConfiguration {
        root,
        generation: initial.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    };
    let exact = serde_json::to_vec(&initial).unwrap();
    for (index, authority) in [Authority::Rust, Authority::Shadow, Authority::Sandbox]
        .into_iter()
        .enumerate()
    {
        let path = std::env::temp_dir().join(format!(
            "retained-task-core-{}-{index}.sqlite",
            std::process::id()
        ));
        assert!(!path.exists());
        Worker::import_whole(path.clone(), initial.clone(), config.clone(), authority)
            .await
            .unwrap()
            .shutdown()
            .await
            .unwrap();
        let worker = Worker::reopen_whole(path.clone(), config.clone(), authority)
            .await
            .unwrap();
        for signed in &retained {
            let receipt = worker
                .client()
                .execute_directory_with_inputs(
                    signed.clone(),
                    ExecutionInputs {
                        version: 99,
                        now: UnixSeconds::new(u64::MAX),
                        seed: [0; 32],
                    },
                )
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                receipt,
                initial.checkpoint.receipts[&signed.call.operation_id].receipt
            );
        }
        for (serial, command) in commands.iter().enumerate() {
            let call = UnsignedContractCallV1::new(
                config.generation,
                OperationId::from_bytes([240 + u8::try_from(serial).unwrap(); 16]),
                origin,
                initial.checkpoint.nonces[&origin],
                UnixSeconds::new(u64::MAX),
                command.clone(),
            )
            .unwrap();
            let signed = SignedContractCallV1 {
                signature: Sr25519Signature::from_bytes(
                    key.sign(&call.signable_bytes().unwrap()).0,
                ),
                call,
            };
            let error = worker
                .client()
                .execute_directory_with_inputs(
                    signed,
                    ExecutionInputs {
                        version: 1,
                        now: UnixSeconds::new(1_900_000_000),
                        seed: [1; 32],
                    },
                )
                .unwrap()
                .result()
                .await
                .unwrap_err();
            assert!(
                matches!(&error, ink_v5_compatibility::worker::Error::Shell(e) if e.is_infrastructure()),
                "{error:?}"
            );
            let observed = worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_vec(&observed).unwrap(),
                exact,
                "{authority:?}: retired task command changed core or history"
            );
        }
        worker.shutdown().await.unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
