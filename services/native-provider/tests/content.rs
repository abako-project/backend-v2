//! Compact execution and durable failure/replay across native store restarts.
#![cfg(feature = "storage-sqlite")]
use native_provider::Provider;
use protocol::{provider_core::MetadataCommitmentV1, *};
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
async fn compact_failure_is_recorded_once_and_replays_after_restart() -> TestResult {
    let key = Keypair::from_secret_key([1; 32])?;
    let root = AccountId32::from_bytes(key.public_key().0);
    let mut token = [0u8; 16];
    getrandom::fill(&mut token).map_err(|_| std::io::Error::other("test path entropy"))?;
    let path = std::env::temp_dir().join(format!(
        "core-metadata-{:032x}.sqlite",
        u128::from_le_bytes(token)
    ));
    let url = format!("sqlite://{}", path.display());
    let provider = Provider::sqlite(&url, root).await?;
    let original = provider.snapshot().await?;
    let operation = OperationId::from_bytes([71; 16]);
    let call = signed(
        &key,
        UnsignedContractCallV1::new(
            original.info.provider_instance_id,
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
    assert_eq!(provider.snapshot().await?, original);
    let generation = provider.info().await?.provider_instance_id;
    drop(provider);
    let reopened = Provider::sqlite(&url, root).await?;
    assert_eq!(
        reopened.execute(call, UnixSeconds::new(1000)).await?,
        receipt
    );
    let next = signed(
        &key,
        UnsignedContractCallV1::new(
            generation,
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

#[cfg(feature = "mock-seed")]
#[tokio::test]
async fn restart_preserves_catalog_deletions_and_never_reseeds_missing_state() -> TestResult {
    let key = Keypair::from_secret_key([1; 32])?;
    let root = AccountId32::from_bytes(key.public_key().0);
    let mut token = [0u8; 16];
    getrandom::fill(&mut token).map_err(|_| std::io::Error::other("test entropy"))?;
    let path = std::env::temp_dir().join(format!(
        "fresh-native-{:032x}.sqlite",
        u128::from_le_bytes(token)
    ));
    let url = format!("sqlite://{}", path.display());
    let provider = Provider::sqlite(&url, root).await?;
    let call = signed(
        &key,
        UnsignedContractCallV1::new(
            provider.info().await?.provider_instance_id,
            OperationId::from_bytes([1; 16]),
            root,
            0,
            UnixSeconds::new(200),
            ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
                kind: CatalogKind::Skill,
                id: 1,
            }),
        )?,
    )?;
    assert_eq!(
        provider.execute(call, UnixSeconds::new(100)).await?.outcome,
        ExecutionOutcome::Success
    );
    let expected = provider.snapshot().await?;
    drop(provider);
    let provider = Provider::sqlite(&url, root).await?;
    assert_eq!(provider.snapshot().await?, expected);
    assert!(!expected.catalog.skills.iter().any(|skill| skill.id == 1));
    drop(provider);
    let pool = sqlx::SqlitePool::connect(&url).await?;
    sqlx::query("DELETE FROM provider_state")
        .execute(&pool)
        .await?;
    pool.close().await;
    assert!(Provider::sqlite(&url, root).await.is_err());
    std::fs::remove_file(&path)?;
    std::fs::write(&path, [])?;
    assert!(Provider::sqlite(&url, root).await.is_err());
    std::fs::remove_file(path)?;
    Ok(())
}
