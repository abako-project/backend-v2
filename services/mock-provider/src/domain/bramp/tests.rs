use parity_scale_codec::{Decode, Encode};
use generated_contracts::{
    AccountId32, CreateDepositRequest, CreateWithdrawalRequest, DepositStatus, Money,
    ProviderCommand, UnixSeconds, WithdrawalStatus,
};

use super::State;
use crate::Result;

const NOW: UnixSeconds = UnixSeconds::new(100);

fn account(value: u8) -> AccountId32 {
    AccountId32::from_bytes([value; 32])
}

#[test]
fn a_deposit_is_immutable_and_can_only_credit_once() -> Result<()> {
    let root = account(1);
    let owner = account(2);
    let stranger = account(3);
    let mut state = State::new(root)?;
    let amount = generated_contracts::KvnAmount::new(Money::new(25))?;
    let effect = state.bramp_command(
        owner,
        &ProviderCommand::CreateDeposit(CreateDepositRequest { amount }),
        NOW,
    )?;
    let id = effect.entity_id.expect("request creates an ID");
    let pending = state.bramp_deposit(owner, id)?;
    assert_eq!(pending.status, DepositStatus::Pending);
    assert_eq!(pending.owner, owner);
    assert_eq!(pending.destination, owner);
    assert_eq!(pending.amount, amount);
    assert_eq!(state.balances.get(&owner), None);
    assert!(state.bramp_deposit(stranger, id).is_err());
    assert!(state
        .bramp_command(
            stranger,
            &ProviderCommand::ConfirmDeposit { deposit_id: id },
            NOW,
        )
        .is_err());
    state.bramp_command(root, &ProviderCommand::ConfirmDeposit { deposit_id: id }, NOW)?;
    assert_eq!(state.bramp_deposit(owner, id)?.status, DepositStatus::Confirmed);
    assert_eq!(state.balances.get(&owner), Some(&Money::new(25)));
    assert_eq!(state.minted_units, Money::new(25));
    assert!(state
        .bramp_command(root, &ProviderCommand::ConfirmDeposit { deposit_id: id }, NOW)
        .is_err());
    assert_eq!(state.balances.get(&owner), Some(&Money::new(25)));
    assert_eq!(state.minted_units, Money::new(25));
    Ok(())
}

#[test]
fn withdrawal_holds_only_free_balance_and_cancels_once() -> Result<()> {
    let root = account(1);
    let owner = account(2);
    let stranger = account(3);
    let mut state = State::new(root)?;
    let deposit = state.bramp_command(
        owner,
        &ProviderCommand::CreateDeposit(CreateDepositRequest {
            amount: generated_contracts::KvnAmount::new(Money::new(25))?,
        }),
        NOW,
    )?;
    state.bramp_command(
        root,
        &ProviderCommand::ConfirmDeposit {
            deposit_id: deposit.entity_id.expect("request creates an ID"),
        },
        NOW,
    )?;
    let too_much = state.bramp_command(
        owner,
        &ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
            amount: generated_contracts::KvnAmount::new(Money::new(26))?,
        }),
        NOW,
    );
    assert!(too_much.is_err());
    assert_eq!(state.balances.get(&owner), Some(&Money::new(25)));
    let effect = state.bramp_command(
        owner,
        &ProviderCommand::CreateWithdrawal(CreateWithdrawalRequest {
            amount: generated_contracts::KvnAmount::new(Money::new(20))?,
        }),
        NOW,
    )?;
    let id = effect.entity_id.expect("request creates an ID");
    assert_eq!(state.balances.get(&owner), Some(&Money::new(5)));
    assert_eq!(state.bramp_withdrawal(owner, id)?.status, WithdrawalStatus::Pending);
    assert!(state.bramp_withdrawal(stranger, id).is_err());
    assert!(state
        .bramp_command(
            stranger,
            &ProviderCommand::CancelWithdrawal { withdrawal_id: id },
            NOW,
        )
        .is_err());
    state.bramp_command(
        root,
        &ProviderCommand::CancelWithdrawal { withdrawal_id: id },
        NOW,
    )?;
    assert_eq!(state.balances.get(&owner), Some(&Money::new(25)));
    assert_eq!(state.bramp_withdrawal(owner, id)?.status, WithdrawalStatus::Cancelled);
    assert!(state
        .bramp_command(
            owner,
            &ProviderCommand::CancelWithdrawal { withdrawal_id: id },
            NOW,
        )
        .is_err());
    assert_eq!(state.balances.get(&owner), Some(&Money::new(25)));
    Ok(())
}

#[test]
fn zero_kvn_cannot_enter_a_request_from_json_or_scale() {
    assert!(generated_contracts::KvnAmount::new(Money::ZERO).is_err());
    assert!(serde_json::from_str::<CreateDepositRequest>(r#"{"amount":"0"}"#).is_err());
    let bytes = Money::ZERO.encode();
    assert!(generated_contracts::KvnAmount::decode(&mut &bytes[..]).is_err());
}
