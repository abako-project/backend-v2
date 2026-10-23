//! Predecessor history compatibility; compact creation is not enabled here.
#![cfg(all(feature = "storage-memory", feature = "storage-sqlite"))]
use generated_contracts::{provider_core::MetadataCommitmentV1, *};
use mock_provider::Provider;
use provider_execution::{Admission, VerifiedCall};
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
async fn predecessor_rejects_fresh_compact_creation_but_restores_and_replays_successor_history()
-> TestResult {
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
    assert_eq!(
        provider
            .execute(call.clone(), UnixSeconds::new(100))
            .await
            .err()
            .ok_or("fresh compact execution accepted")?
            .code(),
        "metadata_cutover_required"
    );
    assert_eq!(
        serde_json::to_vec(&provider.archive().await?)?,
        serde_json::to_vec(&original)?
    );

    // A successor's domain rejection changes shell history, not business facts.
    // Simulate that accepted history independently; this bridge has no new engine.
    let mut shell = original.checkpoint.shell();
    let Admission::Execute(pending) =
        shell.prepare(VerifiedCall::verify(call.clone())?, UnixSeconds::new(100))?
    else {
        return Err("expected new execution admission".into());
    };
    let receipt = shell.complete(*pending, Err("no_available_coordinator"))?;
    let mut rollback = original;
    rollback.checkpoint.nonces = shell.nonces;
    rollback.checkpoint.receipts = shell.receipts;
    rollback.checkpoint.events = shell.events;
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
