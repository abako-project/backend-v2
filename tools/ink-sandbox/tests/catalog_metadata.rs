//! Actual compact catalog/reason execution, native/WASM comparison and V3 restore.
use generated_contracts::{provider_core::*, *};
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV4};
use sp_core::{Pair, sr25519};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
    path::Path,
};
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn actor(value: AccountId32) -> u8 {
    (1..=10).find(|seed| account(*seed) == value).unwrap()
}
fn signed(core: &CoreArchiveV4, who: u8, op: u8, command: ProviderCommand) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        core.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([op; 16]),
        account(who),
        *core.checkpoint.nonces.get(&account(who)).unwrap_or(&0),
        UnixSeconds::new(u64::MAX),
        command,
    )
    .unwrap();
    SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(
            sr25519::Pair::from_seed(&[who; 32])
                .sign(&call.signable_bytes().unwrap())
                .0,
        ),
        call,
    }
}
struct Run {
    worker: Worker,
    core: CoreArchiveV4,
    catalog: BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
    reasons: BTreeMap<PayloadHash, ReasonMetadataRecordV1>,
    calls: Vec<SignedContractCallV1>,
    receipts: Vec<OperationReceipt>,
}
impl Run {
    async fn call(
        &mut self,
        who: u8,
        op: u8,
        original: ProviderCommand,
        expected: ExecutionOutcome,
    ) -> OperationReceipt {
        let envelope = signed(&self.core, who, op, original).call;
        if let Some(record) = CatalogMetadataRecordV1::from_command(
            envelope.provider_instance_id,
            envelope.origin,
            envelope.operation_id,
            &envelope.command,
        ) {
            self.catalog.insert(
                mock_provider::migration::source_hash(&record.hash_input()),
                record,
            );
        }
        if let Some(record) = ReasonMetadataRecordV1::from_command(
            envelope.provider_instance_id,
            envelope.origin,
            envelope.operation_id,
            &envelope.command,
        ) {
            self.reasons.insert(
                mock_provider::migration::source_hash(&record.hash_input()),
                record,
            );
        }
        let command = mock_provider::catalog_content::compact_catalog(&envelope)
            .or_else(|| mock_provider::catalog_content::compact_reason(&envelope))
            .unwrap_or(envelope.command);
        let call = signed(&self.core, who, op, command);
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(2000 + u64::from(op)),
            seed: [op; 32],
        };
        let (native, wanted) =
            mock_provider::ReferenceState::from_archive(account(1), self.core.clone())
                .unwrap()
                .execute(VerifiedCall::verify(call.clone()).unwrap(), inputs.clone())
                .unwrap();
        let receipt = self
            .worker
            .client()
            .execute_directory_with_inputs(call.clone(), inputs)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(receipt, wanted, "operation {op}");
        assert_eq!(receipt.outcome, expected, "operation {op}");
        self.core = self
            .worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&self.core).unwrap(),
            serde_json::to_vec(&native.archive().unwrap()).unwrap(),
            "operation {op}"
        );
        self.calls.push(call);
        self.receipts.push(receipt.clone());
        receipt
    }
    fn restore(&self) -> provider_execution::migration::CoreArchiveV3 {
        mock_provider::migration::to_predecessor(self.core.clone(), &self.catalog, &self.reasons)
            .unwrap()
    }
}
fn private_json(path: &Path, value: &impl serde::Serialize) {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path).unwrap();
    serde_json::to_writer(&mut f, value).unwrap();
    f.sync_all().unwrap();
}
fn ndjson<H: serde::Serialize, T: serde::Serialize>(
    path: &Path,
    header: H,
    records: impl Iterator<Item = T>,
) {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path).unwrap();
    serde_json::to_writer(&mut f, &header).unwrap();
    f.write_all(b"\n").unwrap();
    for record in records {
        serde_json::to_writer(&mut f, &record).unwrap();
        f.write_all(b"\n").unwrap();
    }
    f.sync_all().unwrap();
}
fn export(run: &Run, path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    let saved = &run.core.checkpoint;
    let generation = saved.info.provider_instance_id;
    private_json(&path.join("core.json"), &run.core);
    private_json(&path.join("predecessor.json"), &run.restore());
    private_json(&path.join("calls.json"), &run.calls);
    private_json(&path.join("receipts.json"), &run.receipts);
    private_json(
        &path.join("next.json"),
        &vec![
            signed(
                &run.core,
                1,
                240,
                ProviderCommand::SetScorePolicy(
                    ScorePolicy::new(Percentage::new(50).unwrap(), Percentage::new(50).unwrap())
                        .unwrap(),
                ),
            ),
            signed(
                &run.core,
                9,
                241,
                ProviderCommand::CreateProject(CreateProjectRequest {
                    title: "Further V3 work".into(),
                    description: "After current-state rollback".into(),
                }),
            ),
        ],
    );
    let selected: BTreeSet<_> = saved
        .roles
        .values()
        .chain(saved.skills.values())
        .map(|e| e.content.hash())
        .chain(saved.skill_metadata.values().map(|s| s.origin.hash()))
        .chain(saved.skill_requests.values().map(|r| r.content.hash()))
        .collect();
    ndjson(
        &path.join("catalog.ndjson"),
        CatalogMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
        selected.iter().map(|key| &run.catalog[key]),
    );
    ndjson(
        &path.join("reasons.ndjson"),
        ReasonMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
        saved
            .reasons
            .iter()
            .map(|core| &run.reasons[&core.content.hash()]),
    );
    ndjson(
        &path.join("creation.ndjson"),
        CreationMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
        mock_provider::migration::recover_legacy_creations(&run.core)
            .unwrap()
            .iter(),
    );
    ndjson(
        &path.join("proposals.ndjson"),
        ProposalMetadataHeaderV1 {
            schema_version: 1,
            generation,
        },
        mock_provider::migration::recover_legacy_proposals(&run.core)
            .unwrap()
            .iter(),
    );
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path.join("root-account.hex")).unwrap();
    write!(f, "{}", saved.info.root_account).unwrap();
    f.sync_all().unwrap();
}
#[test]
fn compact_catalog_and_reasons_preserve_current_state_and_actual_receipts() {
    scenario(
        &[Authority::Rust, Authority::Shadow, Authority::Sandbox],
        200_000,
        false,
    );
}
#[test]
fn supported_frozen_rollback_case_executes_its_original_small_reason() {
    scenario(&[Authority::Sandbox], 80, true);
}
fn scenario(authorities: &[Authority], reason_bytes: usize, owner_export: bool) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let predecessor: serde_json::Value = serde_json::from_str(include_str!(
                "../../../services/mock-provider/src/domain/fixtures/catalog-predecessor.json"
            ))
            .unwrap();
            let source = predecessor["planningCases"][1]["archive"]
                .as_str()
                .unwrap()
                .as_bytes();
            let (initial, catalog, reasons) =
                mock_provider::migration::from_predecessor(source, account(1)).unwrap();
            let project = *initial.checkpoint.projects.first_key_value().unwrap().0;
            let client = actor(initial.checkpoint.projects[&project].client);
            let requester = actor(*initial.checkpoint.workers.first_key_value().unwrap().0);
            for (index, authority) in authorities.iter().copied().enumerate() {
                let path = std::env::temp_dir().join(format!(
                    "catalog-v4-{}-{reason_bytes}-{index}.sqlite",
                    std::process::id()
                ));
                let config = WholeConfiguration {
                    root: account(1),
                    generation: initial.checkpoint.info.provider_instance_id,
                    mock_funding: true,
                    gas_limit: WHOLE_GAS,
                };
                let worker =
                    Worker::import_whole(path.clone(), initial.clone(), config.clone(), authority)
                        .await
                        .unwrap();
                let mut run = Run {
                    worker,
                    core: initial.clone(),
                    catalog: catalog
                        .iter()
                        .cloned()
                        .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
                        .collect(),
                    reasons: reasons
                        .iter()
                        .cloned()
                        .map(|r| (mock_provider::migration::source_hash(&r.hash_input()), r))
                        .collect(),
                    calls: vec![],
                    receipts: vec![],
                };
                let upsert = |kind, id, name: String| {
                    ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
                        kind,
                        id,
                        name,
                    })
                };
                let origin = run.core.checkpoint.skill_metadata[&1].origin.clone();
                run.call(
                    1,
                    200,
                    upsert(
                        CatalogKind::Skill,
                        1,
                        format!("Renamed\0{}", "λ".repeat(12_000)),
                    ),
                    ExecutionOutcome::Success,
                )
                .await;
                assert_eq!(run.core.checkpoint.skill_metadata[&1].origin, origin);
                assert_eq!(
                    run.restore().checkpoint.skill_metadata[&1].category,
                    "software"
                );
                run.call(
                    1,
                    201,
                    upsert(CatalogKind::Skill, 100, "Temporary".into()),
                    ExecutionOutcome::Success,
                )
                .await;
                run.call(
                    1,
                    202,
                    ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
                        kind: CatalogKind::Skill,
                        id: 100,
                    }),
                    ExecutionOutcome::Success,
                )
                .await;
                let receipt = run
                    .call(
                        requester,
                        203,
                        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
                            name: "  New  Skill  ".into(),
                            role_ids: vec![3],
                        }),
                        ExecutionOutcome::Success,
                    )
                    .await;
                let request = receipt.created_entity_id.unwrap();
                run.call(
                    1,
                    204,
                    ProviderCommand::DecideSkillRequest(DecideSkillRequest {
                        request_id: request,
                        decision: SkillRequestDecision::Approve,
                    }),
                    ExecutionOutcome::Success,
                )
                .await;
                let skill = run.core.checkpoint.skill_requests[&request]
                    .skill_id
                    .unwrap();
                assert_eq!(skill, 34);
                let origin = run.core.checkpoint.skill_metadata[&skill].origin.clone();
                run.call(
                    1,
                    205,
                    upsert(CatalogKind::Skill, skill, "Renamed approved".into()),
                    ExecutionOutcome::Success,
                )
                .await;
                assert_eq!(run.core.checkpoint.skill_metadata[&skill].origin, origin);
                assert_eq!(
                    run.restore().checkpoint.skill_requests[&request].name,
                    "New  Skill"
                );
                run.call(
                    1,
                    206,
                    ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
                        kind: CatalogKind::Skill,
                        id: skill,
                    }),
                    ExecutionOutcome::Success,
                )
                .await;
                run.call(
                    1,
                    207,
                    upsert(CatalogKind::Skill, skill, "Recreated".into()),
                    ExecutionOutcome::Success,
                )
                .await;
                assert_ne!(run.core.checkpoint.skill_metadata[&skill].origin, origin);
                assert!(
                    run.core.checkpoint.skill_metadata[&skill]
                        .role_ids
                        .is_empty()
                );
                run.call(
                    1,
                    208,
                    upsert(CatalogKind::Role, 1000, "Temporary role".into()),
                    ExecutionOutcome::Success,
                )
                .await;
                let pending = run
                    .call(
                        requester,
                        209,
                        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
                            name: "Pending role".into(),
                            role_ids: vec![1000],
                        }),
                        ExecutionOutcome::Success,
                    )
                    .await
                    .created_entity_id
                    .unwrap();
                run.call(
                    1,
                    210,
                    ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
                        kind: CatalogKind::Role,
                        id: 1000,
                    }),
                    ExecutionOutcome::Success,
                )
                .await;
                run.call(
                    1,
                    211,
                    ProviderCommand::DecideSkillRequest(DecideSkillRequest {
                        request_id: pending,
                        decision: SkillRequestDecision::Approve,
                    }),
                    ExecutionOutcome::Failed("unknown_role".into()),
                )
                .await;
                run.call(
                    client,
                    212,
                    ProviderCommand::DisputePlanning {
                        project_id: project,
                        request: ReasonRequest {
                            reason: format!("Dispute\0{}", "x".repeat(reason_bytes)),
                        },
                    },
                    ExecutionOutcome::Success,
                )
                .await;
                run.call(
                    client,
                    213,
                    ProviderCommand::CancelProject {
                        project_id: project,
                        request: ReasonRequest {
                            reason: "Cancellation\0 preserved".into(),
                        },
                    },
                    ExecutionOutcome::Success,
                )
                .await;
                let restored = run.restore();
                assert_eq!(
                    restored.checkpoint.reasons.len(),
                    initial.checkpoint.reasons.len() + 2
                );
                assert!(
                    restored
                        .checkpoint
                        .reasons
                        .last()
                        .unwrap()
                        .reason
                        .contains('\0')
                );
                assert_eq!(restored.checkpoint.balances, initial.checkpoint.balances);
                assert_eq!(
                    restored.checkpoint.minted_units,
                    initial.checkpoint.minted_units
                );
                let before = serde_json::to_vec(&run.core).unwrap();
                run.worker.shutdown().await.unwrap();
                run.worker = Worker::reopen_whole(path.clone(), config, authority)
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
                    before
                );
                if owner_export && let Some(output) = std::env::var_os("R2B2_ROLLBACK_OUTPUT_DIR") {
                    export(&run, Path::new(&output));
                }
                run.worker.shutdown().await.unwrap();
                std::fs::remove_file(path).unwrap();
            }
        });
}
