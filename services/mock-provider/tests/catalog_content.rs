//! Core owns enforceable facts; this test supplies documents as an external owner.
#![cfg(feature = "storage-memory")]
use generated_contracts::{provider_core::*, *};
use mock_provider::{Provider, migration};
use std::collections::BTreeMap;
use subxt_signer::sr25519::Keypair;
type TestResult = Result<(), Box<dyn std::error::Error>>;
const NOW: UnixSeconds = UnixSeconds::new(1_700_000_000);
async fn apply(
    provider: &Provider,
    key: &Keypair,
    op: u8,
    command: ProviderCommand,
    documents: &mut BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
) -> Result<OperationReceipt, Box<dyn std::error::Error>> {
    let account = AccountId32::from_bytes(key.public_key().0);
    let generation = provider.info().await?.provider_instance_id;
    let operation = OperationId::from_bytes([op; 16]);
    let command = if let Some(record) =
        CatalogMetadataRecordV1::from_command(generation, account, operation, &command)
    {
        let hash = migration::source_hash(&record.hash_input());
        let content = MetadataCommitmentV1 {
            operation_id: operation,
            hash,
        };
        let compact = match &command {
            ProviderCommand::UpsertCatalogEntry(request) => {
                ProviderCommand::UpsertCatalogEntryCore {
                    kind: request.kind,
                    id: request.id,
                    content,
                }
            }
            ProviderCommand::CreateSkillRequest(request) => {
                ProviderCommand::CreateSkillRequestCore {
                    role_ids: request.role_ids.clone(),
                    content,
                }
            }
            _ => return Err("unexpected staging".into()),
        };
        documents.insert(hash, record);
        compact
    } else {
        command
    };
    let call = UnsignedContractCallV1::new(
        generation,
        operation,
        account,
        provider.nonce(account).await?.nonce,
        UnixSeconds::new(NOW.get() + 1000),
        command,
    )?;
    let signed = SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
        call,
    };
    Ok(provider.execute(signed, NOW).await?)
}
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "Keep the actual signed incarnation/role/history scenario together."
)]
async fn compact_catalog_retains_incarnation_roles_history_and_exact_v3_restore() -> TestResult {
    let root = Keypair::from_secret_key([1; 32])?;
    let worker = Keypair::from_secret_key([2; 32])?;
    let root_account = AccountId32::from_bytes(root.public_key().0);
    let provider = Provider::memory(root_account)?;
    let generation = provider.info().await?.provider_instance_id;
    let mut documents: BTreeMap<_, _> = catalog_seed_v1()
        .into_iter()
        .map(|entry| {
            let record = entry.record(generation);
            (migration::source_hash(&record.hash_input()), record)
        })
        .collect();
    let seed_origin = provider.archive().await?.checkpoint.skill_metadata[&1]
        .origin
        .clone();
    let upsert = |kind, id, name: &str| {
        ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
            kind,
            id,
            name: name.into(),
        })
    };
    assert_eq!(
        apply(
            &provider,
            &root,
            1,
            upsert(
                CatalogKind::Skill,
                1,
                &format!("Renamed\0{}", "x".repeat(22_000))
            ),
            &mut documents
        )
        .await?
        .outcome,
        ExecutionOutcome::Success
    );
    let archive = provider.archive().await?;
    assert_eq!(archive.checkpoint.skill_metadata[&1].origin, seed_origin);
    let old = migration::to_predecessor(archive, &documents, &BTreeMap::new())?;
    assert_eq!(old.checkpoint.skill_metadata[&1].category, "software");
    assert!(old.checkpoint.skills[&1].name.contains('\0'));
    apply(
        &provider,
        &root,
        2,
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: 1,
        }),
        &mut documents,
    )
    .await?;
    apply(
        &provider,
        &root,
        3,
        upsert(CatalogKind::Skill, 1, "Recreated"),
        &mut documents,
    )
    .await?;
    let archive = provider.archive().await?;
    assert_ne!(archive.checkpoint.skill_metadata[&1].origin, seed_origin);
    assert!(archive.checkpoint.skill_metadata[&1].role_ids.is_empty());
    assert_eq!(
        migration::to_predecessor(archive, &documents, &BTreeMap::new())?
            .checkpoint
            .skill_metadata[&1]
            .category,
        "requested"
    );
    apply(
        &provider,
        &worker,
        4,
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Worker".into(),
            qualifications: Qualifications {
                role_ids: vec![1],
                skill_ids: vec![],
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: vec![],
            },
        }),
        &mut documents,
    )
    .await?;
    let requested = apply(
        &provider,
        &worker,
        5,
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "  New  Skill  ".into(),
            role_ids: vec![1],
        }),
        &mut documents,
    )
    .await?;
    let request_id = requested.created_entity_id.ok_or("missing request ID")?;
    assert_eq!(provider.archive().await?.checkpoint.skills.len(), 33);
    apply(
        &provider,
        &root,
        6,
        ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id,
            decision: SkillRequestDecision::Approve,
        }),
        &mut documents,
    )
    .await?;
    let archive = provider.archive().await?;
    let request = &archive.checkpoint.skill_requests[&request_id];
    let skill_id = request.skill_id.ok_or("missing assigned skill")?;
    assert_eq!(skill_id, 34);
    assert_eq!(
        archive.checkpoint.skills[&skill_id].content,
        request.content
    );
    assert_eq!(
        archive.checkpoint.skill_metadata[&skill_id].origin,
        request.content
    );
    let origin = request.content.clone();
    apply(
        &provider,
        &root,
        7,
        upsert(CatalogKind::Skill, skill_id, "Changed"),
        &mut documents,
    )
    .await?;
    let archive = provider.archive().await?;
    assert_eq!(archive.checkpoint.skill_metadata[&skill_id].origin, origin);
    assert_eq!(archive.checkpoint.skill_metadata[&skill_id].role_ids, [1]);
    let old = migration::to_predecessor(archive, &documents, &BTreeMap::new())?;
    assert_eq!(
        old.checkpoint.skill_requests[&request_id].name,
        "New  Skill"
    );
    assert_eq!(old.checkpoint.skills[&skill_id].name, "Changed");
    apply(
        &provider,
        &root,
        8,
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: skill_id,
        }),
        &mut documents,
    )
    .await?;
    apply(
        &provider,
        &root,
        9,
        upsert(CatalogKind::Skill, skill_id, "Reused"),
        &mut documents,
    )
    .await?;
    let archive = provider.archive().await?;
    assert_ne!(archive.checkpoint.skill_metadata[&skill_id].origin, origin);
    assert!(
        archive.checkpoint.skill_metadata[&skill_id]
            .role_ids
            .is_empty()
    );
    assert_eq!(
        archive.checkpoint.skill_requests[&request_id].status,
        SkillRequestStatus::Approved
    );
    migration::to_predecessor(archive, &documents, &BTreeMap::new())?;
    apply(
        &provider,
        &root,
        10,
        upsert(CatalogKind::Role, 10, "Temporary"),
        &mut documents,
    )
    .await?;
    let pending = apply(
        &provider,
        &worker,
        11,
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "Role race".into(),
            role_ids: vec![10],
        }),
        &mut documents,
    )
    .await?
    .created_entity_id
    .ok_or("missing request")?;
    let deleted = apply(
        &provider,
        &root,
        12,
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Role,
            id: 10,
        }),
        &mut documents,
    )
    .await?;
    assert_eq!(
        deleted.outcome,
        ExecutionOutcome::Success,
        "pending requests do not guard role deletion"
    );
    let rejected = apply(
        &provider,
        &root,
        13,
        ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: pending,
            decision: SkillRequestDecision::Approve,
        }),
        &mut documents,
    )
    .await?;
    assert_eq!(
        rejected.outcome,
        ExecutionOutcome::Failed("unknown_role".into())
    );
    let archive = provider.archive().await?;
    assert_eq!(
        archive.checkpoint.skill_requests[&pending].status,
        SkillRequestStatus::Pending
    );
    migration::to_predecessor(archive, &documents, &BTreeMap::new())?;
    Ok(())
}
