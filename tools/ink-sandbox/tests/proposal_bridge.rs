//! V2 rollback target understands future proposal bytes without executing them.
use generated_contracts::{provider_core::*, *};
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, Error, WholeConfiguration, Worker},
};
use provider_execution::{Admission, Effect, ExecutionInputs, VerifiedCall};
use sp_core::{Pair, sr25519};
use std::path::PathBuf;

struct Database(PathBuf);
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}
#[test]
fn all_authorities_restore_future_success_receipts_but_reject_new_compact_execution() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let key = sr25519::Pair::from_seed(&[1; 32]);
            let root = AccountId32::from_bytes(key.public().0);
            let original = mock_provider::Provider::memory(root)
                .unwrap()
                .archive()
                .await
                .unwrap();
            let project_id = EntityId::from_bytes([5; 16]);
            let proposal_id = EntityId::from_bytes([6; 16]);
            let week = Week::new(2026, 40).unwrap();
            let terms = ProposalTerms {
                milestones: vec![MilestoneTerms {
                    key: 1,
                    window: WeekWindow::new(week, week).unwrap(),
                    coordinator_fee: Money::ZERO,
                    coordinator_minutes: Minutes::new(0),
                    requirements: vec![],
                }],
            };
            let mut shell = original.checkpoint.shell();
            let mut calls = Vec::new();
            let mut receipts = Vec::new();
            for index in 0u8..3 {
                let operation_id = OperationId::from_bytes([100 + index; 16]);
                let content = MetadataCommitmentV1 {
                    operation_id,
                    hash: PayloadHash::from_bytes([4; 32]),
                };
                let command = match index {
                    0 => ProviderCommand::CreateProposalCore {
                        project_id,
                        proposal: terms.clone(),
                        content,
                    },
                    1 => ProviderCommand::UpdateProposalCore {
                        project_id,
                        proposal_id,
                        proposal: terms.clone(),
                        content,
                    },
                    _ => ProviderCommand::RequestProposalChangesCore {
                        project_id,
                        proposal_id,
                        content,
                    },
                };
                let call = UnsignedContractCallV1::new(
                    shell.info.provider_instance_id,
                    operation_id,
                    root,
                    u64::from(index),
                    UnixSeconds::new(200),
                    command,
                )
                .unwrap();
                let signed = SignedContractCallV1 {
                    signature: Sr25519Signature::from_bytes(
                        key.sign(&call.signable_bytes().unwrap()).0,
                    ),
                    call,
                };
                let Admission::Execute(pending) = shell
                    .prepare_with_inputs(
                        VerifiedCall::verify(signed.clone()).unwrap(),
                        ExecutionInputs {
                            version: 1,
                            now: UnixSeconds::new(100),
                            seed: [1; 32],
                        },
                    )
                    .unwrap()
                else {
                    panic!("fixture cannot replay");
                };
                // Simulates the V2 rehydrated history. Real success is proved by the
                // subsequent V3 cutover test, not by this predecessor fixture.
                let kind = match index {
                    0 => DomainEventKind::ProposalCreated,
                    1 => DomainEventKind::ProposalUpdated,
                    _ => DomainEventKind::ProposalChangesRequested,
                };
                receipts.push(
                    shell
                        .complete(*pending, Ok(Effect::new(kind, Some(proposal_id), root)))
                        .unwrap(),
                );
                calls.push(signed);
            }
            let mut retained = original;
            retained.checkpoint.nonces = shell.nonces;
            retained.checkpoint.receipts = shell.receipts;
            retained.checkpoint.events = shell.events;
            let config = WholeConfiguration {
                root,
                generation: retained.checkpoint.info.provider_instance_id,
                mock_funding: true,
                gas_limit: WHOLE_GAS,
            };
            for (index, authority) in [Authority::Rust, Authority::Shadow, Authority::Sandbox]
                .into_iter()
                .enumerate()
            {
                let database = Database(std::env::temp_dir().join(format!(
                    "proposal-v2-bridge-{}-{index}.sqlite",
                    std::process::id()
                )));
                let worker = Worker::import_whole(
                    database.0.clone(),
                    retained.clone(),
                    config.clone(),
                    authority,
                )
                .await
                .unwrap();
                for (call, receipt) in calls.iter().zip(&receipts) {
                    assert_eq!(
                        worker
                            .client()
                            .execute_directory_with_inputs(
                                call.clone(),
                                ExecutionInputs {
                                    version: 99,
                                    now: UnixSeconds::new(1000),
                                    seed: [2; 32]
                                }
                            )
                            .unwrap()
                            .result()
                            .await
                            .unwrap(),
                        *receipt
                    );
                }
                let before = serde_json::to_vec(
                    &worker
                        .client()
                        .export_archive()
                        .unwrap()
                        .result()
                        .await
                        .unwrap(),
                )
                .unwrap();
                let mut fresh = calls[0].call.clone();
                fresh.operation_id = OperationId::from_bytes([200; 16]);
                fresh.nonce = 3;
                let ProviderCommand::CreateProposalCore { content, .. } = &mut fresh.command else {
                    panic!("fixture");
                };
                content.operation_id = fresh.operation_id;
                let fresh = SignedContractCallV1 {
                    signature: Sr25519Signature::from_bytes(
                        key.sign(&fresh.signable_bytes().unwrap()).0,
                    ),
                    call: fresh,
                };
                assert!(matches!(
                    worker
                        .client()
                        .execute_directory_with_inputs(
                            fresh,
                            ExecutionInputs {
                                version: 1,
                                now: UnixSeconds::new(100),
                                seed: [1; 32]
                            }
                        )
                        .unwrap()
                        .result()
                        .await,
                    Err(Error::Invariant("metadata_cutover_required"))
                ));
                assert_eq!(
                    serde_json::to_vec(
                        &worker
                            .client()
                            .export_archive()
                            .unwrap()
                            .result()
                            .await
                            .unwrap()
                    )
                    .unwrap(),
                    before
                );
                worker.shutdown().await.unwrap();
                let reopened = Worker::reopen_whole(database.0.clone(), config.clone(), authority)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_vec(
                        &reopened
                            .client()
                            .export_archive()
                            .unwrap()
                            .result()
                            .await
                            .unwrap()
                    )
                    .unwrap(),
                    before
                );
                reopened.shutdown().await.unwrap();
            }
        });
}
