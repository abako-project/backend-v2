//! Mock-only KVN ramp requests. The provider's outer transaction commits these
//! records with balances, receipts, nonces and events as one unit.

use std::collections::{BTreeMap, BTreeSet};

use generated_contracts::{
    AccountId32, DepositStatus, DepositView, DomainEventKind, EntityId, Money, ProviderCommand,
    UnixSeconds, WithdrawalStatus, WithdrawalView,
};
use serde::{Deserialize, Serialize};

use super::{Effect, State};
use crate::{Error, Result, random_id, require};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct BrampState {
    deposits: BTreeMap<EntityId, DepositView>,
    withdrawals: BTreeMap<EntityId, WithdrawalView>,
}

impl State {
    pub(super) fn bramp_command(
        &mut self,
        origin: AccountId32,
        command: &ProviderCommand,
        now: UnixSeconds,
    ) -> Result<Effect> {
        match command {
            ProviderCommand::CreateDeposit(request) => {
                let deposit_id = self.bramp.new_id()?;
                self.bramp.deposits.insert(
                    deposit_id,
                    DepositView {
                        deposit_id,
                        owner: origin,
                        destination: origin,
                        amount: request.amount,
                        status: DepositStatus::Pending,
                        created_at: now,
                        confirmed_at: None,
                    },
                );
                Ok(Effect::new(
                    DomainEventKind::DepositRequested,
                    Some(deposit_id),
                    origin,
                ))
            }
            ProviderCommand::ConfirmDeposit { deposit_id } => {
                self.root(origin)?;
                let deposit = self
                    .bramp
                    .deposits
                    .get(deposit_id)
                    .ok_or_else(|| Error::domain("deposit_not_found"))?;
                require(
                    deposit.status == DepositStatus::Pending,
                    "deposit_already_confirmed",
                )?;
                let destination = deposit.destination;
                let amount = deposit.amount.money();
                let next_supply = self.minted_units.checked_add(amount)?;
                self.credit(destination, amount)?;
                self.minted_units = next_supply;
                let deposit = self
                    .bramp
                    .deposits
                    .get_mut(deposit_id)
                    .ok_or_else(Error::internal)?;
                deposit.status = DepositStatus::Confirmed;
                deposit.confirmed_at = Some(now);
                let mut effect = Effect::new(
                    DomainEventKind::DepositConfirmed,
                    Some(*deposit_id),
                    destination,
                );
                effect.recipients.insert(origin);
                Ok(effect)
            }
            ProviderCommand::CreateWithdrawal(request) => {
                let withdrawal_id = self.bramp.new_id()?;
                self.debit(origin, request.amount.money())?;
                self.bramp.withdrawals.insert(
                    withdrawal_id,
                    WithdrawalView {
                        withdrawal_id,
                        owner: origin,
                        amount: request.amount,
                        status: WithdrawalStatus::Pending,
                        created_at: now,
                        cancelled_at: None,
                    },
                );
                Ok(Effect::new(
                    DomainEventKind::WithdrawalRequested,
                    Some(withdrawal_id),
                    origin,
                ))
            }
            ProviderCommand::CancelWithdrawal { withdrawal_id } => {
                let withdrawal = self
                    .bramp
                    .withdrawals
                    .get(withdrawal_id)
                    .ok_or_else(|| Error::domain("withdrawal_not_found"))?;
                require(
                    origin == withdrawal.owner || origin == self.info.root_account,
                    "withdrawal_owner_or_system_required",
                )?;
                require(
                    withdrawal.status == WithdrawalStatus::Pending,
                    "withdrawal_already_cancelled",
                )?;
                let owner = withdrawal.owner;
                let amount = withdrawal.amount.money();
                self.credit(owner, amount)?;
                let withdrawal = self
                    .bramp
                    .withdrawals
                    .get_mut(withdrawal_id)
                    .ok_or_else(Error::internal)?;
                withdrawal.status = WithdrawalStatus::Cancelled;
                withdrawal.cancelled_at = Some(now);
                let mut effect = Effect::new(
                    DomainEventKind::WithdrawalCancelled,
                    Some(*withdrawal_id),
                    owner,
                );
                effect.recipients.insert(origin);
                Ok(effect)
            }
            _ => Err(Error::bad("invalid_bramp_message")),
        }
    }

    /// Read authorization is checked at the provider, not only at the adapter.
    pub(crate) fn bramp_deposit(
        &self,
        origin: AccountId32,
        deposit_id: EntityId,
    ) -> Result<DepositView> {
        let deposit = self
            .bramp
            .deposits
            .get(&deposit_id)
            .ok_or_else(|| Error::domain("deposit_not_found"))?;
        require(
            origin == deposit.owner || origin == self.info.root_account,
            "deposit_owner_or_system_required",
        )?;
        Ok(deposit.clone())
    }

    /// Read authorization is checked at the provider, not only at the adapter.
    pub(crate) fn bramp_withdrawal(
        &self,
        origin: AccountId32,
        withdrawal_id: EntityId,
    ) -> Result<WithdrawalView> {
        let withdrawal = self
            .bramp
            .withdrawals
            .get(&withdrawal_id)
            .ok_or_else(|| Error::domain("withdrawal_not_found"))?;
        require(
            origin == withdrawal.owner || origin == self.info.root_account,
            "withdrawal_owner_or_system_required",
        )?;
        Ok(withdrawal.clone())
    }

    /// Validate persisted requests and return the value excluded from free
    /// balances by still-pending withdrawal holds.
    pub(super) fn validate_bramp(&self, identities: &mut BTreeSet<EntityId>) -> Result<Money> {
        for (id, deposit) in &self.bramp.deposits {
            require(
                *id == deposit.deposit_id
                    && identities.insert(*id)
                    && deposit.owner == deposit.destination
                    && deposit.created_at <= deposit.confirmed_at.unwrap_or(deposit.created_at)
                    && (deposit.status == DepositStatus::Confirmed)
                        == deposit.confirmed_at.is_some(),
                "invalid_deposit_state",
            )?;
        }
        let mut held = Money::ZERO;
        for (id, withdrawal) in &self.bramp.withdrawals {
            require(
                *id == withdrawal.withdrawal_id
                    && identities.insert(*id)
                    && withdrawal.created_at
                        <= withdrawal.cancelled_at.unwrap_or(withdrawal.created_at)
                    && (withdrawal.status == WithdrawalStatus::Cancelled)
                        == withdrawal.cancelled_at.is_some(),
                "invalid_withdrawal_state",
            )?;
            if withdrawal.status == WithdrawalStatus::Pending {
                held = held.checked_add(withdrawal.amount.money())?;
            }
        }
        Ok(held)
    }
}

impl BrampState {
    fn new_id(&self) -> Result<EntityId> {
        let id = random_id()?;
        require(
            !self.deposits.contains_key(&id) && !self.withdrawals.contains_key(&id),
            "duplicate_entity",
        )?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
