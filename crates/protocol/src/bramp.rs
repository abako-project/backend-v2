//! Typed wire data for the mock payment ramp.

use crate::{AccountId32, EntityId, Money, UnixSeconds};
use parity_scale_codec::{Decode, Encode, Input};
use serde::{Deserialize, Serialize};

use crate::ContractError;

/// A strictly positive integer amount; the operation determines the asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Encode, Serialize, Deserialize)]
#[serde(try_from = "Money", into = "Money")]
pub struct PositiveAmount(Money);

impl PositiveAmount {
    /// Reject zero before a request can be signed or persisted.
    pub fn new(amount: Money) -> Result<Self, ContractError> {
        if amount == Money::ZERO {
            return Err(ContractError::Invalid("positive amount"));
        }
        Ok(Self(amount))
    }

    /// Return the integer amount for checked ledger arithmetic.
    #[must_use]
    pub const fn money(self) -> Money {
        self.0
    }
}

impl TryFrom<Money> for PositiveAmount {
    type Error = ContractError;

    fn try_from(amount: Money) -> Result<Self, Self::Error> {
        Self::new(amount)
    }
}

impl From<PositiveAmount> for Money {
    fn from(amount: PositiveAmount) -> Self {
        amount.money()
    }
}

impl Decode for PositiveAmount {
    fn decode<I: Input>(input: &mut I) -> Result<Self, parity_scale_codec::Error> {
        Self::new(Money::decode(input)?).map_err(|_| "amount must be positive".into())
    }
}

/// Requesting a simulated deposit changes no balance until confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDepositRequest {
    /// Fixed amount credited after operator confirmation.
    pub amount: PositiveAmount,
}

/// Requesting a simulated withdrawal moves free payment units into a hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWithdrawalRequest {
    /// Fixed amount held while the request remains pending.
    pub amount: PositiveAmount,
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
    /// Payment units are held outside the owner's free balance.
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
    /// Payment amount agreed at creation.
    pub amount: PositiveAmount,
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
    /// Payment amount held at creation.
    pub amount: PositiveAmount,
    /// Current request state.
    pub status: WithdrawalStatus,
    /// Provider Unix timestamp of creation.
    pub created_at: UnixSeconds,
    /// Provider Unix timestamp of cancellation, if any.
    pub cancelled_at: Option<UnixSeconds>,
}
