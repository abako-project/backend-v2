//! Typed wire data for the mock-only KVN ramp.

use domain_primitives::{AccountId32, EntityId, Money, UnixSeconds};
use parity_scale_codec::{Decode, Encode, Input};
use serde::{Deserialize, Serialize};

use crate::ContractError;

/// A strictly positive amount of asset 1 (KVN), in integer token units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Encode, Serialize, Deserialize)]
#[serde(try_from = "Money", into = "Money")]
pub struct KvnAmount(Money);

impl KvnAmount {
    /// Reject zero before a request can be signed or persisted.
    pub fn new(amount: Money) -> Result<Self, ContractError> {
        if amount == Money::ZERO {
            return Err(ContractError::Invalid("positive KVN amount"));
        }
        Ok(Self(amount))
    }

    /// Return the integer amount for checked ledger arithmetic.
    #[must_use]
    pub const fn money(self) -> Money {
        self.0
    }
}

impl TryFrom<Money> for KvnAmount {
    type Error = ContractError;

    fn try_from(amount: Money) -> Result<Self, Self::Error> {
        Self::new(amount)
    }
}

impl From<KvnAmount> for Money {
    fn from(amount: KvnAmount) -> Self {
        amount.money()
    }
}

impl Decode for KvnAmount {
    fn decode<I: Input>(input: &mut I) -> Result<Self, parity_scale_codec::Error> {
        Self::new(Money::decode(input)?).map_err(|_| "KVN amount must be positive".into())
    }
}

/// Requesting a simulated deposit changes no balance until confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDepositRequest {
    /// Fixed amount credited after operator confirmation.
    pub amount: KvnAmount,
}

/// Requesting a simulated withdrawal moves free KVN into a hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWithdrawalRequest {
    /// Fixed amount held while the request remains pending.
    pub amount: KvnAmount,
}

/// A deposit may be credited once; it cannot be cancelled in this proof of concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum DepositStatus {
    /// No spendable balance has changed.
    Pending,
    /// The operator credited the original amount to the original destination.
    Confirmed,
}

/// A withdrawal cannot settle to a bank in this proof of concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum WithdrawalStatus {
    /// KVN is held outside the owner's free balance.
    Pending,
    /// The hold was returned to the owner's free balance.
    Cancelled,
}

/// Immutable deposit terms with an explicit mutable lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DepositView {
    /// Provider-generated request identity.
    pub deposit_id: EntityId,
    /// Account that requested the deposit.
    pub owner: AccountId32,
    /// Custodial account to credit; fixed to the owner's account.
    pub destination: AccountId32,
    /// KVN amount agreed at creation.
    pub amount: KvnAmount,
    /// Current request state.
    pub status: DepositStatus,
    /// Provider Unix timestamp of creation.
    pub created_at: UnixSeconds,
    /// Provider Unix timestamp of the only confirmation, if any.
    pub confirmed_at: Option<UnixSeconds>,
}

/// Immutable withdrawal terms with a pending hold or a cancelled hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WithdrawalView {
    /// Provider-generated request identity.
    pub withdrawal_id: EntityId,
    /// Account whose free balance was debited.
    pub owner: AccountId32,
    /// KVN amount held at creation.
    pub amount: KvnAmount,
    /// Current request state.
    pub status: WithdrawalStatus,
    /// Provider Unix timestamp of creation.
    pub created_at: UnixSeconds,
    /// Provider Unix timestamp of cancellation, if any.
    pub cancelled_at: Option<UnixSeconds>,
}
