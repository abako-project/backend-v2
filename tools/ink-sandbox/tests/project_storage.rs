//! Real WASM growth, parent reordering, shrink and reuse with ordinary Project cells.
use generated_contracts::{provider_core::*, *};
use ink_sandbox::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::ReferenceState;
use parity_scale_codec::Encode;
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV5};
use sp_core::{Pair, sr25519};

#[tokio::test]
async fn task_growth_parent_reorder_shrink_and_reuse_match_native_after_restart() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed-v5.json"
    ))
    .unwrap();
    let mut initial: CoreArchiveV5 = serde_json::from_str(
        corpus["archives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["beforeStep"] == 35)
            .unwrap()["archive"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let root = initial.checkpoint.info.root_account;
    let members: Vec<_> = initial.checkpoint.workers.keys().take(2).copied().collect();
    assert_eq!(members.len(), 2);
    let first = initial.checkpoint.next_task_id;
    let project = initial.checkpoint.projects.values_mut().next().unwrap();
    let project_id = project.project_id;
    let origin = project.coordinator;
    let proposal = &mut project.proposals[0];
    let proposal_id = proposal.proposal_id;
    assert_eq!(proposal.status, ProposalStatus::PendingApproval);
    assert_eq!(proposal.milestones.len(), 2);
    let original_terms = ProposalTerms {
        milestones: proposal
            .milestones
            .iter()
            .map(|m| m.definition.clone())
            .collect(),
    };
    let original_key = proposal.milestones[0].definition.key;
    let storage = proposal.milestones[0].task_storage.task_storage_id;
    let tasks = &mut proposal.milestones[0].task_storage.tasks;
    for offset in 0..600u32 {
        tasks.push(TaskCore {
            task_id: first.checked_add(offset).unwrap(),
            assignees: if offset % 2 == 0 {
                members.clone()
            } else {
                members.iter().rev().copied().collect()
            },
        });
    }
    assert!(tasks.encode().len() > 16 * 1024);
    initial.checkpoint.next_task_id = first.checked_add(600).unwrap();
    let mut reference = ReferenceState::from_archive(root, initial.clone()).unwrap();
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
    let path = std::env::temp_dir().join(format!(
        "project-ordinary-growth-{}.sqlite",
        std::process::id()
    ));
    assert!(!path.exists());
    let worker = Worker::import_whole(
        path.clone(),
        initial,
        configuration.clone(),
        Authority::Sandbox,
    )
    .await
    .unwrap();
    for sequence in 0..7u8 {
        let current = reference.archive().unwrap();
        let proposal = &current.checkpoint.projects[&project_id].proposals[0];
        let operation_id = OperationId::from_bytes([210 + sequence; 16]);
        let content = MetadataCommitmentV1 {
            operation_id,
            hash: PayloadHash::from_bytes([sequence; 32]),
        };
        let command = match sequence {
            0 => ProviderCommand::WithdrawProposal {
                project_id,
                proposal_id,
                expected_revision: proposal.revision,
            },
            1 => ProviderCommand::SetTaskAssigneesCore {
                project_id,
                task_storage_id: storage,
                task_id: first,
                assignees: vec![members[0]],
            },
            2 => ProviderCommand::DeleteTask {
                project_id,
                task_storage_id: storage,
                task_id: first + 300,
                expected_revision: proposal.revision,
            },
            3 | 4 => {
                let mut terms = ProposalTerms {
                    milestones: proposal
                        .milestones
                        .iter()
                        .map(|m| m.definition.clone())
                        .collect(),
                };
                if sequence == 3 {
                    terms.milestones.reverse();
                } else {
                    terms.milestones.truncate(1);
                }
                ProviderCommand::UpdateProposalCore {
                    project_id,
                    proposal_id,
                    proposal: terms,
                    content,
                }
            }
            5 => ProviderCommand::UpdateProposalCore {
                project_id,
                proposal_id,
                proposal: original_terms.clone(),
                content,
            },
            6 => ProviderCommand::CreateTaskCore {
                project_id,
                task_storage_id: proposal
                    .milestones
                    .iter()
                    .find(|m| m.definition.key == original_key)
                    .unwrap()
                    .task_storage
                    .task_storage_id,
                assignees: vec![members[1]],
            },
            _ => unreachable!(),
        };
        let call = UnsignedContractCallV1::new(
            configuration.generation,
            operation_id,
            origin,
            reference.nonce(origin).nonce,
            UnixSeconds::new(1_900_000_000),
            command,
        )
        .unwrap();
        let signed = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0),
            call,
        };
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(1_800_000_000),
            seed: [sequence; 32],
        };
        let (next, expected) = reference
            .execute(
                VerifiedCall::verify(signed.clone()).unwrap(),
                inputs.clone(),
            )
            .unwrap();
        assert_eq!(
            expected.outcome,
            ExecutionOutcome::Success,
            "native step {sequence}"
        );
        reference = next;
        let receipt = worker
            .client()
            .execute_directory_with_inputs(signed, inputs)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt, expected, "WASM step {sequence}");
        let diagnostics = worker
            .client()
            .whole_diagnostics()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(diagnostics.comparison, "matched");
        assert_eq!(diagnostics.comparisons, i64::from(sequence) + 1);
        let actual = worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(reference.archive().unwrap()).unwrap(),
            "archive step {sequence}"
        );
    }
    worker.shutdown().await.unwrap();
    let worker = Worker::reopen_whole(path.clone(), configuration, Authority::Sandbox)
        .await
        .unwrap();
    let actual = worker
        .client()
        .export_archive()
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(reference.archive().unwrap()).unwrap()
    );
    let diagnostics = worker
        .client()
        .whole_diagnostics()
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(diagnostics.comparison, "matched");
    assert_eq!(diagnostics.comparisons, 7);
    worker.shutdown().await.unwrap();
    std::fs::remove_file(path).unwrap();
}
