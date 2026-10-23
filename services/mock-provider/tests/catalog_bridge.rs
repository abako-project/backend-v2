//! Supported predecessor decodes/replays future bytes, without fresh execution.
//! Successful history here is a rehydrated fixture, not compact business parity.
#![cfg(all(feature = "storage-memory", feature = "storage-sqlite"))]
use generated_contracts::{provider_core::*, *};
use mock_provider::Provider;
use provider_execution::{Admission, Effect, ExecutionInputs, VerifiedCall};
use subxt_signer::sr25519::Keypair;

type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn future_catalog_reason_calls_replay_after_import_but_never_execute_fresh() -> TestResult {
    let key = Keypair::from_secret_key([1; 32])?;
    let root = AccountId32::from_bytes(key.public_key().0);
    let provider = Provider::memory(root)?;
    let original = provider.archive().await?;
    let mut shell = original.checkpoint.shell();
    for index in 0u8..4 {
        let operation_id = OperationId::from_bytes([100 + index; 16]);
        let content = MetadataCommitmentV1 {
            operation_id,
            hash: PayloadHash::from_bytes([4; 32]),
        };
        let project_id = EntityId::from_bytes([5; 16]);
        let command = match index {
            0 => ProviderCommand::UpsertCatalogEntryCore {
                kind: CatalogKind::Skill,
                id: 99,
                content,
            },
            1 => ProviderCommand::CreateSkillRequestCore {
                role_ids: vec![1],
                content,
            },
            2 => ProviderCommand::CancelProjectCore {
                project_id,
                content,
            },
            _ => ProviderCommand::DisputePlanningCore {
                project_id,
                content,
            },
        };
        // The live predecessor sees a fresh operation and must leave all state alone.
        let fresh = UnsignedContractCallV1::new(
            original.checkpoint.info.provider_instance_id,
            operation_id,
            root,
            0,
            UnixSeconds::new(200),
            command.clone(),
        )?;
        let fresh = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(key.sign(&fresh.signable_bytes()?).0),
            call: fresh,
        };
        assert_eq!(
            provider
                .execute(fresh, UnixSeconds::new(100))
                .await
                .err()
                .ok_or("executed future command")?
                .code(),
            "metadata_cutover_required"
        );
        assert_eq!(
            serde_json::to_vec(&provider.archive().await?)?,
            serde_json::to_vec(&original)?
        );
        let call = UnsignedContractCallV1::new(
            shell.info.provider_instance_id,
            operation_id,
            root,
            u64::from(index),
            UnixSeconds::new(200),
            command,
        )?;
        let signed = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
            call,
        };
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(100),
            seed: [1; 32],
        };
        let Admission::Execute(pending) =
            shell.prepare_with_inputs(VerifiedCall::verify(signed.clone())?, inputs)?
        else {
            return Err("unexpected fixture replay".into());
        };
        let kind = match index {
            0 => DomainEventKind::CatalogUpdated,
            1 => DomainEventKind::SkillRequested,
            2 => DomainEventKind::ProjectCancelled,
            _ => DomainEventKind::PlanningDisputed,
        };
        let receipt = shell.complete(*pending, Ok(Effect::new(kind, Some(project_id), root)))?;
        let mut retained = original.clone();
        retained.checkpoint.nonces = shell.nonces.clone();
        retained.checkpoint.receipts = shell.receipts.clone();
        retained.checkpoint.events = shell.events.clone();
        let imported = Provider::memory_from_archive(root, retained.clone()).await?;
        assert_eq!(
            imported.execute(signed, UnixSeconds::new(1000)).await?,
            receipt
        );
        assert_eq!(
            serde_json::to_vec(&imported.archive().await?)?,
            serde_json::to_vec(&retained)?
        );
    }
    Ok(())
}
