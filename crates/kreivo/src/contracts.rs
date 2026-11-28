//! pallet-contracts through its runtime API and its `call` extrinsic.

use blake2::{Blake2b, Digest, digest::consts::U32};
use parity_scale_codec::{Decode, Encode};
use protocol::AccountId32;
use sube::{DynValue, PreparedCall};

use crate::{Error, Result, worker::State};

/// Bit of `ReturnFlags` an ink! message sets when it reverts.
const REVERT: u32 = 1;
/// Gas a dry-run may use. Without a limit the runtime allows one block, and a
/// read of a large collection then fails as if the contract were at fault.
/// This is far more than a transaction can declare: compare
/// [`DryRun::gas_required`] with the chain's limits before submitting.
const DRY_RUN_GAS: Weight = Weight {
    ref_time: 2_000_000_000_000,
    proof_size: 128 * 1024 * 1024,
};

/// Runtime weight: computation time and proof size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Encode, Decode)]
pub struct Weight {
    /// Computation, in picoseconds of reference hardware.
    #[codec(compact)]
    pub ref_time: u64,
    /// Storage proof the block must carry, in bytes.
    #[codec(compact)]
    pub proof_size: u64,
}

impl Weight {
    /// This weight with a tenth added, for the difference between a dry-run and
    /// the block the call lands in.
    #[must_use]
    pub const fn with_margin(self) -> Self {
        Self {
            ref_time: self.ref_time.saturating_add(self.ref_time / 10),
            proof_size: self.proof_size.saturating_add(self.proof_size / 10),
        }
    }
}

/// What a contract message returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Returned {
    /// Whether the message reverted its state changes.
    pub reverted: bool,
    /// The SCALE-encoded return value.
    pub data: Vec<u8>,
}

/// A contract message run without being submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DryRun {
    /// Gas a transaction must allow for the same message.
    pub gas_required: Weight,
    /// Native balance the caller would lock as storage deposit.
    pub storage_deposit: u128,
    /// The return value, or the dispatch error when the call could not run.
    pub outcome: core::result::Result<Returned, String>,
}

/// The input of an ink! message or constructor: the selector of `name`, then
/// its SCALE-encoded arguments.
#[must_use]
pub fn message(name: &str, arguments: impl Encode) -> Vec<u8> {
    let hash: [u8; 32] = Blake2b::<U32>::digest(name.as_bytes()).into();
    let mut input = hash[..4].to_vec();
    arguments.encode_to(&mut input);
    input
}

#[derive(Decode)]
pub(crate) struct ExecReturnValue {
    pub(crate) flags: u32,
    pub(crate) data: Vec<u8>,
}

/// Decode a `ContractResult` up to its `result`. `decode_ok` reads the `Ok`
/// value; the events that follow need runtime types and are left unread.
pub(crate) fn decode_result<T>(
    bytes: &[u8],
    decode_ok: impl FnOnce(&mut &[u8]) -> core::result::Result<T, parity_scale_codec::Error>,
) -> Result<(Weight, u128, core::result::Result<T, String>)> {
    let malformed = |error: parity_scale_codec::Error| Error::Decode(error.to_string());
    let input = &mut &bytes[..];
    let _consumed = Weight::decode(input).map_err(malformed)?;
    let required = Weight::decode(input).map_err(malformed)?;
    // StorageDeposit::{Refund, Charge}(Balance): only a charge is owed.
    let charged = u8::decode(input).map_err(malformed)? == 1;
    let amount = u128::decode(input).map_err(malformed)?;
    let deposit = if charged { amount } else { 0 };
    let _debug = Vec::<u8>::decode(input).map_err(malformed)?;
    let outcome = match u8::decode(input).map_err(malformed)? {
        0 => Ok(decode_ok(input).map_err(malformed)?),
        1 => Err(format!("dispatch error 0x{}", hex(input))),
        other => return Err(Error::Decode(format!("result tag {other}"))),
    };
    Ok((required, deposit, outcome))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write;
    bytes.iter().fold(String::new(), |mut text, byte| {
        // Writing to a String cannot fail.
        let _ = write!(text, "{byte:02x}");
        text
    })
}

pub(crate) fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex(bytes))
}

/// The parameters of `ContractsApi::call(origin, dest, value, gas_limit,
/// storage_deposit_limit, input_data)` for a dry-run.
pub(crate) fn call_parameters(caller: AccountId32, contract: AccountId32, input: &[u8]) -> Vec<u8> {
    (
        caller.as_bytes(),
        contract.as_bytes(),
        0_u128,
        Some(DRY_RUN_GAS),
        None::<u128>,
        input,
    )
        .encode()
}

impl State {
    pub(crate) async fn dry_run(
        &mut self,
        caller: AccountId32,
        contract: AccountId32,
        input: &[u8],
    ) -> Result<DryRun> {
        let parameters = call_parameters(caller, contract, input);
        let output = self.runtime_api("ContractsApi_call", &parameters).await?;
        let (gas_required, storage_deposit, outcome) =
            decode_result(&output, |input| ExecReturnValue::decode(input))?;
        Ok(DryRun {
            gas_required,
            storage_deposit,
            outcome: outcome.map(|value| Returned {
                reverted: value.flags & REVERT != 0,
                data: value.data,
            }),
        })
    }

    /// `Contracts.call` with no value attached. `deposit` is the most the
    /// call may lock as storage deposit; without one the chain's default
    /// limit applies.
    pub(crate) fn contracts_call(
        &self,
        contract: AccountId32,
        input: &[u8],
        gas: Weight,
        deposit: Option<u128>,
    ) -> Result<PreparedCall> {
        let body = DynValue::obj(&[
            (
                "dest",
                DynValue::obj(&[("Id", DynValue::from(contract.into_bytes()))]),
            ),
            ("value", DynValue::U128(0)),
            (
                "gas_limit",
                DynValue::obj(&[
                    ("ref_time", DynValue::U64(gas.ref_time)),
                    ("proof_size", DynValue::U64(gas.proof_size)),
                ]),
            ),
            // A bare value for an `Option` field is encoded as `Some(value)`.
            (
                "storage_deposit_limit",
                deposit.map_or(DynValue::Null, DynValue::U128),
            ),
            ("data", DynValue::Bytes(input.to_vec())),
        ]);
        Ok(self.sube.prepare_call("contracts/call", &body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_input_is_the_selector_then_the_arguments() {
        // Selectors recorded in the pinned Dao metadata.
        assert_eq!(message("new", ()), [0x9b, 0xae, 0x9d, 0x5e]);
        assert_eq!(
            message("configure_accounting", true),
            [0x7a, 0x6b, 0x80, 0x76, 1]
        );
    }

    #[test]
    fn dry_run_results_decode_up_to_the_events() -> Result<()> {
        let weight = |ref_time, proof_size| Weight {
            ref_time,
            proof_size,
        };
        let reverted = (
            weight(7, 9),
            weight(70, 90),
            1_u8, // StorageDeposit::Charge
            5_u128,
            Vec::<u8>::new(),
            0_u8, // Ok
            REVERT,
            vec![1_u8, 2],
            0xff_u8, // the unread events
        )
            .encode();
        let (gas, deposit, outcome) =
            decode_result(&reverted, |input| ExecReturnValue::decode(input))?;
        assert_eq!((gas, deposit), (weight(70, 90), 5));
        assert!(matches!(
            outcome,
            Ok(ExecReturnValue { flags: REVERT, data }) if data == [1, 2]
        ));

        let failed = (
            weight(0, 0),
            weight(0, 0),
            0_u8, // StorageDeposit::Refund
            3_u128,
            Vec::<u8>::new(),
            1_u8, // Err
            [3_u8, 80, 2],
        )
            .encode();
        let (_, deposit, outcome) = decode_result(&failed, |input| ExecReturnValue::decode(input))?;
        assert_eq!(deposit, 0);
        assert_eq!(outcome.err().as_deref(), Some("dispatch error 0x035002"));
        Ok(())
    }

    #[test]
    fn truncated_results_are_errors() {
        assert!(matches!(
            decode_result(&[1, 2], |input| ExecReturnValue::decode(input)),
            Err(Error::Decode(_))
        ));
    }

    #[test]
    fn margin_adds_a_tenth() {
        let weight = Weight {
            ref_time: 1_000,
            proof_size: u64::MAX,
        };
        assert_eq!(
            weight.with_margin(),
            Weight {
                ref_time: 1_100,
                proof_size: u64::MAX
            }
        );
    }
}
