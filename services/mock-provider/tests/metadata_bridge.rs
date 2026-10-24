//! Compact execution and durable failure/replay across native store restarts.
#![cfg(all(feature = "storage-memory", feature = "storage-sqlite"))]
use generated_contracts::{provider_core::MetadataCommitmentV1, *};
use mock_provider::Provider;
use subxt_signer::sr25519::Keypair;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn signed(
    key: &Keypair,
    call: UnsignedContractCallV1,
) -> Result<SignedContractCallV1, ContractError> {
    Ok(SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0),
        call,
    })
}
#[tokio::test]
async fn compact_failure_is_recorded_once_and_replays_after_import_and_restart() -> TestResult {
    let key = Keypair::from_secret_key([1; 32])?;
    let root = AccountId32::from_bytes(key.public_key().0);
    let provider = Provider::memory(root)?;
    let original = provider.archive().await?;
    let operation = OperationId::from_bytes([71; 16]);
    let call = signed(
        &key,
        UnsignedContractCallV1::new(
            original.checkpoint.info.provider_instance_id,
            operation,
            root,
            0,
            UnixSeconds::new(200),
            ProviderCommand::CreateProjectCore(MetadataCommitmentV1 {
                operation_id: operation,
                hash: PayloadHash::from_bytes([72; 32]),
            }),
        )?,
    )?;
    let receipt = provider
        .execute(call.clone(), UnixSeconds::new(100))
        .await?;
    assert_eq!(
        receipt.outcome,
        ExecutionOutcome::Failed("no_available_candidate".into())
    );
    let rollback = provider.archive().await?;
    assert_eq!(
        serde_json::to_vec(&rollback.checkpoint.workers)?,
        serde_json::to_vec(&original.checkpoint.workers)?
    );
    assert_eq!(
        serde_json::to_vec(&rollback.checkpoint.projects)?,
        serde_json::to_vec(&original.checkpoint.projects)?
    );
    let mut token = [0u8; 16];
    getrandom::fill(&mut token).map_err(|_| std::io::Error::other("test path entropy"))?;
    let path = std::env::temp_dir().join(format!(
        "core-metadata-bridge-{:032x}.sqlite",
        u128::from_le_bytes(token)
    ));
    let url = format!("sqlite://{}", path.display());
    let restored = Provider::sqlite_from_archive(&url, root, rollback.clone()).await?;
    assert_eq!(
        restored
            .execute(call.clone(), UnixSeconds::new(1000))
            .await?,
        receipt
    );
    assert_eq!(
        serde_json::to_vec(&restored.archive().await?)?,
        serde_json::to_vec(&rollback)?
    );
    drop(restored);
    let reopened = Provider::sqlite(&url, root).await?;
    assert_eq!(
        reopened.execute(call, UnixSeconds::new(1000)).await?,
        receipt
    );
    let next = signed(
        &key,
        UnsignedContractCallV1::new(
            rollback.checkpoint.info.provider_instance_id,
            OperationId::from_bytes([73; 16]),
            root,
            1,
            UnixSeconds::new(200),
            ProviderCommand::SetScorePolicy(ScorePolicy::new(
                Percentage::new(50)?,
                Percentage::new(50)?,
            )?),
        )?,
    )?;
    assert_eq!(
        reopened.execute(next, UnixSeconds::new(101)).await?.outcome,
        ExecutionOutcome::Success
    );
    drop(reopened);
    std::fs::remove_file(path)?;
    Ok(())
}
