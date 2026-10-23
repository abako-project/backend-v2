//! Successful compact proposal writes, current-state rollback and real replay.
use generated_contracts::{provider_core::*, *};
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use parity_scale_codec::Encode;
use provider_execution::{
    ExecutionInputs,
    migration::{CoreArchiveV3, CoreArchiveV4},
};
use sp_core::{Pair, sr25519};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn actor(account_id: AccountId32) -> u8 {
    (1..=10).find(|seed| account(*seed) == account_id).unwrap()
}
fn config(core: &CoreArchiveV4) -> WholeConfiguration {
    WholeConfiguration {
        root: core.checkpoint.info.root_account,
        generation: core.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    }
}
fn signed(
    core: &CoreArchiveV4,
    actor: u8,
    op: u8,
    command: ProviderCommand,
) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        core.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([op; 16]),
        account(actor),
        *core.checkpoint.nonces.get(&account(actor)).unwrap_or(&0),
        UnixSeconds::new(u64::MAX),
        command,
    )
    .unwrap();
    SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(
            sr25519::Pair::from_seed(&[actor; 32])
                .sign(&call.signable_bytes().unwrap())
                .0,
        ),
        call,
    }
}
struct Run {
    worker: Worker,
    core: CoreArchiveV4,
    records: BTreeMap<PayloadHash, ProposalMetadataRecordV1>,
    calls: Vec<SignedContractCallV1>,
    receipts: Vec<OperationReceipt>,
}
impl Run {
    fn stage(&mut self, actor: u8, op: u8, command: ProviderCommand) -> ProviderCommand {
        let call = signed(&self.core, actor, op, command).call;
        let record = ProposalMetadataRecordV1::from_command(
            call.provider_instance_id,
            call.origin,
            call.operation_id,
            &call.command,
        )
        .unwrap();
        self.records.insert(
            mock_provider::migration::source_hash(&record.hash_input()),
            record,
        );
        mock_provider::proposal::compact_proposal(&call).unwrap()
    }
    async fn call(
        &mut self,
        actor: u8,
        op: u8,
        command: ProviderCommand,
        expected: ExecutionOutcome,
    ) -> OperationReceipt {
        let call = signed(&self.core, actor, op, command);
        let receipt = self
            .worker
            .client()
            .execute_directory_with_inputs(
                call.clone(),
                ExecutionInputs {
                    version: 1,
                    now: UnixSeconds::new(1000 + u64::from(op)),
                    seed: [op; 32],
                },
            )
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt.outcome, expected, "operation {op}");
        self.calls.push(call);
        self.receipts.push(receipt.clone());
        self.core = self
            .worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        receipt
    }
    fn selected(&self) -> BTreeMap<PayloadHash, ProposalMetadataRecordV1> {
        let mut records = BTreeMap::new();
        for project in self.core.checkpoint.projects.values() {
            for proposal in &project.proposals {
                for reference in
                    std::iter::once(&proposal.content).chain(proposal.change_request.as_ref())
                {
                    records.insert(reference.hash(), self.records[&reference.hash()].clone());
                }
            }
        }
        records
    }
}
fn private(path: &Path) -> File {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).unwrap()
}
fn json(path: &Path, value: &impl serde::Serialize) {
    let mut f = private(path);
    serde_json::to_writer(&mut f, value).unwrap();
    f.sync_all().unwrap();
}
fn ndjson<T: serde::Serialize>(
    path: &Path,
    header: &impl serde::Serialize,
    records: impl Iterator<Item = T>,
) {
    let mut f = private(path);
    serde_json::to_writer(&mut f, header).unwrap();
    f.write_all(b"\n").unwrap();
    for r in records {
        serde_json::to_writer(&mut f, &r).unwrap();
        f.write_all(b"\n").unwrap();
    }
    f.sync_all().unwrap();
}
fn export(run: &Run, old: &CoreArchiveV3, next: &[SignedContractCallV1], directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    json(&directory.join("core.json"), &run.core);
    json(&directory.join("predecessor.json"), old);
    json(&directory.join("calls.json"), &run.calls);
    json(&directory.join("receipts.json"), &run.receipts);
    json(&directory.join("next.json"), &next);
    ndjson(
        &directory.join("metadata.ndjson"),
        &ProposalMetadataHeaderV1 {
            schema_version: 1,
            generation: run.core.checkpoint.info.provider_instance_id,
        },
        run.selected().values(),
    );
    let root_records = mock_provider::migration::recover_legacy_creations(&run.core).unwrap();
    ndjson(
        &directory.join("creation.ndjson"),
        &CreationMetadataHeaderV1 {
            schema_version: 1,
            generation: run.core.checkpoint.info.provider_instance_id,
        },
        root_records.iter(),
    );
    let mut f = private(&directory.join("root-account.hex"));
    write!(f, "{}", run.core.checkpoint.info.root_account).unwrap();
    f.sync_all().unwrap();
}
#[test]
fn compact_proposals_bind_execution_time_policy_preserve_revisions_and_roll_back_current_state() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(rehearsal());
}
async fn rehearsal() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/catalog-predecessor.json"
    ))
    .unwrap();
    let source = reference["archives"][1]["archive"].as_str().unwrap();
    let previous: CoreArchiveV3 = serde_json::from_str(source).unwrap();
    let project_id = *previous.checkpoint.projects.first_key_value().unwrap().0;
    let project = &previous.checkpoint.projects[&project_id];
    let coordinator = actor(project.coordinator);
    let client = actor(project.client);
    let original_proposal = project.proposals[0].proposal_id;
    let (core, catalog_records, reason_records) =
        mock_provider::migration::from_predecessor(source.as_bytes(), account(1)).unwrap();
    let records: BTreeMap<_, _> = mock_provider::migration::recover_legacy_proposals(&core)
        .unwrap()
        .into_iter()
        .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
        .collect();
    let mut definition = project.proposals[0]
        .definition(&records[&project.proposals[0].content.hash()])
        .unwrap();
    definition.title = "Committed\0proposal".into();
    definition.description = "λ".repeat(40_000);
    for milestone in &mut definition.milestones {
        milestone.title = format!("Milestone\0{}", milestone.key);
        milestone.coordinator_fee = Money::ZERO;
        for r in &mut milestone.requirements {
            r.budget = Money::ZERO;
            r.minutes = Minutes::new(37);
        }
        milestone.requirements[0].budget = Money::new(100);
    }
    let path = std::env::temp_dir().join(format!("proposal-success-{}.sqlite", std::process::id()));
    let worker = Worker::import_whole(
        path.clone(),
        core.clone(),
        config(&core),
        Authority::Sandbox,
    )
    .await
    .unwrap();
    let mut run = Run {
        worker,
        core,
        records,
        calls: vec![],
        receipts: vec![],
    };
    run.call(
        1,
        199,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: account(client),
            amount: Money::new(1000),
        }),
        ExecutionOutcome::Success,
    )
    .await;
    let funded_balances = run.core.checkpoint.balances.clone();
    let funded_issuance = run.core.checkpoint.minted_units;
    assert_eq!(
        funded_issuance,
        previous
            .checkpoint
            .minted_units
            .checked_add(Money::new(1000))
            .unwrap()
    );
    // Stage while policy=10, execute after root changes it. No predicted revision.
    let create = run.stage(
        coordinator,
        201,
        ProviderCommand::CreateProposal {
            project_id,
            proposal: definition.clone(),
        },
    );
    run.call(
        1,
        200,
        ProviderCommand::SetCompensationPolicy(CompensationPolicy {
            coordinator_percent: Percentage::new(20).unwrap(),
        }),
        ExecutionOutcome::Success,
    )
    .await;
    let receipt = run
        .call(coordinator, 201, create, ExecutionOutcome::Success)
        .await;
    let proposal_id = receipt.created_entity_id.unwrap();
    let proposal = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    assert_eq!(proposal.revision, 0);
    assert_eq!(
        proposal
            .compensation_policy
            .as_ref()
            .unwrap()
            .coordinator_percent,
        Percentage::new(20).unwrap()
    );
    assert!(proposal.encoded_size() < 16 * 1024);
    assert!(
        serde_json::to_value(proposal)
            .unwrap()
            .get("description")
            .is_none()
    );
    let identities: Vec<_> = proposal
        .milestones
        .iter()
        .map(|m| {
            (
                m.definition.key,
                m.milestone_id,
                m.task_storage.task_storage_id,
            )
        })
        .collect();
    let initial_ref = proposal.content.clone();
    run.call(
        1,
        202,
        ProviderCommand::SetCompensationPolicy(CompensationPolicy {
            coordinator_percent: Percentage::new(70).unwrap(),
        }),
        ExecutionOutcome::Success,
    )
    .await;
    let proposal = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    let shown = proposal
        .definition(&run.records[&proposal.content.hash()])
        .unwrap();
    assert_eq!(shown.title, definition.title);
    assert_eq!(shown.description, definition.description);
    for m in &shown.milestones {
        assert_eq!(m.coordinator_fee, Money::new(20));
        assert_eq!(
            m.requirements.iter().map(|r| r.budget.units()).sum::<u64>(),
            80
        );
    }
    run.call(
        coordinator,
        203,
        ProviderCommand::SetProposalDelivery {
            project_id,
            proposal_id,
            request: ProposalDeliveryRequest {
                expected_revision: 0,
                delivery: DeliveryPreference::WithinOneMonth {},
            },
        },
        ExecutionOutcome::Success,
    )
    .await;
    run.call(
        coordinator,
        204,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
        ExecutionOutcome::Success,
    )
    .await;
    let changes = run.stage(
        client,
        205,
        ProviderCommand::RequestProposalChanges {
            project_id,
            proposal_id,
            request: RequestChangesRequest {
                reference: "Requested\0changes".into(),
            },
        },
    );
    run.call(client, 205, changes, ExecutionOutcome::Success)
        .await;
    let proposal = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    assert_eq!(proposal.revision, 3);
    assert_eq!(proposal.content, initial_ref);
    let change_ref = proposal.change_request.clone();
    definition.description.push_str(" updated");
    let update = run.stage(
        coordinator,
        206,
        ProviderCommand::UpdateProposal {
            project_id,
            proposal_id,
            proposal: definition.clone(),
        },
    );
    run.call(coordinator, 206, update, ExecutionOutcome::Success)
        .await;
    let proposal = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    assert_eq!(proposal.revision, 4);
    assert_eq!(proposal.change_request, change_ref);
    assert_eq!(
        proposal
            .compensation_policy
            .as_ref()
            .unwrap()
            .coordinator_percent,
        Percentage::new(70).unwrap()
    );
    assert_eq!(
        proposal
            .milestones
            .iter()
            .map(|m| (
                m.definition.key,
                m.milestone_id,
                m.task_storage.task_storage_id
            ))
            .collect::<Vec<_>>(),
        identities
    );
    // Direct legacy operations have no API record until explicit owner recovery.
    run.call(
        client,
        207,
        ProviderCommand::RequestProposalChanges {
            project_id,
            proposal_id: original_proposal,
            request: RequestChangesRequest {
                reference: "Recover this request".into(),
            },
        },
        ExecutionOutcome::Success,
    )
    .await;
    let legacy = previous.checkpoint.projects[&project_id].proposals[0].clone();
    let legacy_definition = legacy
        .definition(&run.records[&legacy.content.hash()])
        .unwrap();
    run.call(
        coordinator,
        208,
        ProviderCommand::UpdateProposal {
            project_id,
            proposal_id: original_proposal,
            proposal: legacy_definition.clone(),
        },
        ExecutionOutcome::Success,
    )
    .await;
    run.call(
        client,
        209,
        ProviderCommand::UpdateProposal {
            project_id,
            proposal_id: original_proposal,
            proposal: legacy_definition,
        },
        ExecutionOutcome::Failed("coordinator_required".into()),
    )
    .await;
    let recovered = mock_provider::migration::recover_legacy_proposals(&run.core).unwrap();
    for op in [207, 208] {
        assert!(recovered.iter().any(|r|matches!(r.provenance,ProposalProvenanceV1::Operation{operation_id,..} if operation_id==OperationId::from_bytes([op;16]))));
    }
    assert!(!recovered.iter().any(|r|matches!(r.provenance,ProposalProvenanceV1::Operation{operation_id,..} if operation_id==OperationId::from_bytes([209;16]))));
    for record in recovered {
        run.records.insert(
            mock_provider::migration::source_hash(&record.hash_input()),
            record,
        );
    }
    run.call(
        coordinator,
        210,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
        ExecutionOutcome::Success,
    )
    .await;
    run.call(
        coordinator,
        211,
        ProviderCommand::WithdrawProposal {
            project_id,
            proposal_id,
            expected_revision: 5,
        },
        ExecutionOutcome::Success,
    )
    .await;
    let selected = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    assert_eq!(selected.revision, 6);
    assert_eq!(selected.change_request, change_ref);
    let selected_ref = selected.content.clone();
    assert_eq!(run.core.checkpoint.balances, funded_balances);
    assert_eq!(
        run.core.checkpoint.next_task_id,
        previous.checkpoint.next_task_id
    );
    run.call(
        coordinator,
        212,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
        ExecutionOutcome::Success,
    )
    .await;
    run.call(
        client,
        213,
        ProviderCommand::ApproveExecution {
            project_id,
            proposal_id,
            expected_revision: 7,
        },
        ExecutionOutcome::Success,
    )
    .await;
    let selected = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    assert_eq!(selected.status, ProposalStatus::Approved);
    assert_eq!(selected.content, selected_ref);
    assert_eq!(selected.change_request, change_ref);
    assert_eq!(run.core.checkpoint.minted_units, funded_issuance);
    let records = run.selected();
    let catalog_records = catalog_records
        .into_iter()
        .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
        .collect();
    let reason_records = reason_records
        .into_iter()
        .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
        .collect();
    let old = mock_provider::migration::to_predecessor(
        run.core.clone(),
        &catalog_records,
        &reason_records,
    )
    .unwrap();
    let restored = old.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    let current_proposal = run.core.checkpoint.projects[&project_id]
        .proposals
        .iter()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap();
    let compact_input = run
        .calls
        .iter()
        .find(|c| matches!(c.call.command, ProviderCommand::CreateProposalCore { .. }))
        .unwrap()
        .call
        .command
        .encoded_size();
    println!(
        "Proposal logical SCALE: V3 core {} bytes; V4 core {} bytes; compact creation input {} bytes",
        restored.encoded_size(),
        current_proposal.encoded_size(),
        compact_input
    );
    let restored_definition = restored
        .definition(&records[&restored.content.hash()])
        .unwrap();
    assert_eq!(restored_definition.title, definition.title);
    assert_eq!(restored_definition.description, definition.description);
    let change = records
        .get(&restored.change_request.as_ref().unwrap().hash())
        .unwrap();
    assert_eq!(
        change.content,
        ProposalContentV1::ChangeRequest("Requested\0changes".into())
    );
    for m in &restored.milestones {
        assert_eq!(m.definition.coordinator_fee, Money::new(70));
        assert_eq!(
            m.definition
                .requirements
                .iter()
                .map(|r| r.budget.units())
                .sum::<u64>(),
            30
        );
    }
    assert_eq!(old.checkpoint.balances, run.core.checkpoint.balances);
    assert_eq!(old.checkpoint.nonces, run.core.checkpoint.nonces);
    assert_eq!(old.checkpoint.events, run.core.checkpoint.events);
    assert_eq!(
        old.checkpoint.next_task_id,
        run.core.checkpoint.next_task_id
    );
    assert_eq!(
        serde_json::to_vec(&old.checkpoint.receipts).unwrap(),
        serde_json::to_vec(&run.core.checkpoint.receipts).unwrap()
    );
    let mut missing = records.clone();
    missing.remove(&selected_ref.hash());
    assert!(
        restored
            .definition(missing.get(&selected_ref.hash()).unwrap_or(change))
            .is_err()
    );
    let mut tampered = records.clone();
    tampered.get_mut(&selected_ref.hash()).unwrap().content =
        ProposalContentV1::ChangeRequest("wrong kind".into());
    assert!(
        restored
            .definition(&tampered[&selected_ref.hash()])
            .is_err()
    );
    let mut swapped = run.core.clone();
    let project = swapped.checkpoint.projects.get_mut(&project_id).unwrap();
    let bad = project
        .proposals
        .iter()
        .find(|p| p.proposal_id == original_proposal)
        .unwrap()
        .content
        .clone();
    project
        .proposals
        .iter_mut()
        .find(|p| p.proposal_id == proposal_id)
        .unwrap()
        .content = bad;
    assert!(mock_provider::ReferenceState::from_archive(account(1), swapped).is_err());
    let snapshot = serde_json::to_vec(&run.core).unwrap();
    let configuration = config(&run.core);
    run.worker.shutdown().await.unwrap();
    run.worker = Worker::reopen_whole(path.clone(), configuration, Authority::Sandbox)
        .await
        .unwrap();
    for (call, receipt) in run.calls.iter().zip(&run.receipts) {
        assert_eq!(
            run.worker
                .client()
                .execute_directory_with_inputs(
                    call.clone(),
                    ExecutionInputs {
                        version: 99,
                        now: UnixSeconds::new(u64::MAX),
                        seed: [0; 32]
                    }
                )
                .unwrap()
                .result()
                .await
                .unwrap(),
            *receipt
        );
    }
    assert_eq!(
        serde_json::to_vec(
            &run.worker
                .client()
                .export_archive()
                .unwrap()
                .result()
                .await
                .unwrap()
        )
        .unwrap(),
        snapshot
    );
    assert_eq!(
        run.worker
            .client()
            .whole_diagnostics()
            .unwrap()
            .result()
            .await
            .unwrap()
            .comparison,
        "matched"
    );
    let next = vec![
        signed(
            &run.core,
            1,
            220,
            ProviderCommand::SetScorePolicy(
                ScorePolicy::new(Percentage::new(50).unwrap(), Percentage::new(50).unwrap())
                    .unwrap(),
            ),
        ),
        signed(
            &run.core,
            9,
            221,
            ProviderCommand::CreateProject(CreateProjectRequest {
                title: "Further V2 work".into(),
                description: "After current-state rollback".into(),
            }),
        ),
    ];
    if let Some(output) = std::env::var_os("R2B_PROPOSAL_PRESERVATION_OUTPUT_DIR") {
        export(&run, &old, &next, Path::new(&output));
    }
    run.worker.shutdown().await.unwrap();
    std::fs::remove_file(path).unwrap();
}
