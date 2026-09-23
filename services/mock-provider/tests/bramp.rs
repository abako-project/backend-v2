//! Signed Bramp flows run unchanged against both mock storage backends.

use std::{
    error::Error,
    sync::atomic::{AtomicU64, Ordering},
};

use generated_contracts::{
    AccountId32, CreateDepositRequest, CreateWithdrawalRequest, DepositStatus, EntityId,
    ExecutionOutcome, KvnAmount, Money, OperationId, ProviderCommand, SignedContractCallV1,
    Sr25519Signature, UnixSeconds, UnsignedContractCallV1, WithdrawalStatus,
};
use mock_provider::Provider;
use subxt_signer::sr25519::Keypair;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const NOW: UnixSeconds = UnixSeconds::new(1_788_912_000);
static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

fn key(seed: u8) -> TestResult<Keypair> {
    Ok(Keypair::from_secret_key([seed; 32])?)
}

fn account(key: &Keypair) -> AccountId32 {
    AccountId32::from_bytes(key.public_key().0)
}

async fn signed(
    provider: &Provider,
    key: &Keypair,
    command: ProviderCommand,
) -> TestResult<SignedContractCallV1> {
    let mut id = [0; 16];
    id[..8].copy_from_slice(&NEXT_OPERATION.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let call = UnsignedContractCallV1::new(
        provider.info().await?.provider_instance_id,
        OperationId::from_bytes(id),
        account(key),
        provider.nonce(account(key)).await?.nonce,
        UnixSeconds::new(NOW.get() + 300),
        command,
    )?;
    let signature = Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
    Ok(SignedContractCallV1 { call, signature })
}

async fn execute(
    provider: &Provider,
    key: &Keypair,
    command: ProviderCommand,
) -> TestResult<generated_contracts::OperationReceipt> {
    Ok(provider
        .execute(signed(provider, key, command).await?, NOW)
        .await?)
}

fn created_id(receipt: &generated_contracts::OperationReceipt) -> TestResult<EntityId> {
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    receipt
        .created_entity_id
        .ok_or_else(|| "missing created ID".into())
}

async fn run_bramp_flow(provider: Provider) -> TestResult {
    let root = key(1)?;
    let owner = key(2)?;
    let stranger = key(3)?;
    let amount = KvnAmount::new(Money::new(25))?;
    let deposit_id = created_id(
        &execute(
            &provider,
            &owner,
            ProviderCommand::CreateDeposit(CreateDepositRequest { amount }),
        )
        .await?,
    )?;
    assert_eq!(
        provider
            .bramp_deposit(account(&owner), deposit_id)
            .await?
            .status,
        DepositStatus::Pending,
    );
    assert!(
        provider
            .bramp_deposit(account(&stranger), deposit_id)
            .await
            .is_err()
    );
    assert_eq!(
        execute(
            &provider,
            &stranger,
            ProviderCommand::ConfirmDeposit { deposit_id },
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("system_origin_required".into()),
    );
    let confirm = signed(
        &provider,
        &root,
        ProviderCommand::ConfirmDeposit { deposit_id },
    )
    .await?;
    let first = provider.execute(confirm.clone(), NOW).await?;
    assert_eq!(first.outcome, ExecutionOutcome::Success);
    assert_eq!(provider.execute(confirm, NOW).await?, first);
    assert_eq!(
        execute(
            &provider,
            &root,
            ProviderCommand::ConfirmDeposit { deposit_id }
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("deposit_already_confirmed".into()),
    );
    assert_eq!(
        provider
            .bramp_deposit(account(&owner), deposit_id)
            .await?
            .status,
        DepositStatus::Confirmed,
    );
    let balance = provider
        .snapshot()
        .await?
        .balances
        .into_iter()
        .find(|item| item.account == account(&owner))
        .ok_or("missing owner balance")?;
    assert_eq!(balance.available, Money::new(25));

    assert_eq!(
        execute(
            &provider,
            &owner,
            ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
                amount: KvnAmount::new(Money::new(26))?,
            }),
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("insufficient_balance".into()),
    );
    let withdrawal_id = created_id(
        &execute(
            &provider,
            &owner,
            ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
                amount: KvnAmount::new(Money::new(20))?,
            }),
        )
        .await?,
    )?;
    assert_eq!(
        provider
            .bramp_withdrawal(account(&owner), withdrawal_id)
            .await?
            .status,
        WithdrawalStatus::Pending,
    );
    assert!(
        provider
            .bramp_withdrawal(account(&stranger), withdrawal_id)
            .await
            .is_err()
    );
    assert_eq!(
        execute(
            &provider,
            &stranger,
            ProviderCommand::CancelWithdrawal { withdrawal_id },
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("withdrawal_owner_or_system_required".into()),
    );
    assert_eq!(
        execute(
            &provider,
            &root,
            ProviderCommand::CancelWithdrawal { withdrawal_id },
        )
        .await?
        .outcome,
        ExecutionOutcome::Success,
    );
    assert_eq!(
        execute(
            &provider,
            &owner,
            ProviderCommand::CancelWithdrawal { withdrawal_id },
        )
        .await?
        .outcome,
        ExecutionOutcome::Failed("withdrawal_already_cancelled".into()),
    );
    let balance = provider
        .snapshot()
        .await?
        .balances
        .into_iter()
        .find(|item| item.account == account(&owner))
        .ok_or("missing owner balance")?;
    assert_eq!(balance.available, Money::new(25));
    Ok(())
}

#[cfg(feature = "storage-memory")]
#[tokio::test]
async fn bramp_memory() -> TestResult {
    run_bramp_flow(Provider::memory(account(&key(1)?))?).await
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn bramp_sqlite() -> TestResult {
    run_bramp_flow(Provider::sqlite("sqlite::memory:", account(&key(1)?)).await?).await
}
