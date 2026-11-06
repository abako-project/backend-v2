//! Kreivo extension 0, functions 0/1/2 at db0ccca77dd0f8a437bfb07769f8afacb726d56a.
//! Uses real pallet-assets transfers with the same Preserve policy as Kreivo.
use crate::{Account, Assets, Runtime};
use frame_support::{
    traits::{
        fungibles::{Inspect, Mutate},
        tokens::Preservation,
    },
    weights::Weight,
};
use pallet_contracts::chain_extension::{ChainExtension, Environment, Ext, InitState, RetVal};
use parity_scale_codec::{DecodeAll, Encode};
use sp_runtime::DispatchError;

#[derive(Default)]
pub struct KreivoAssets;

impl ChainExtension<Runtime> for KreivoAssets {
    fn call<E: Ext<T = Runtime>>(
        &mut self,
        env: Environment<E, InitState>,
    ) -> Result<RetVal, DispatchError> {
        let function = env.func_id();
        if env.ext_id() != 0 || function > 2 {
            return Err(DispatchError::Other("unsupported Kreivo extension"));
        }
        let mut env = env.buf_in_buf_out();
        // Fixed maximum SCALE input: Here(u32), u128, AccountId32 = 53 bytes.
        // Conservative finite proof charge, not a production benchmark weight.
        env.charge_weight(Weight::from_parts(1_000_000_000, 64 * 1024))?;
        if env.in_len() > 53 {
            return Err(DispatchError::Other("oversized asset input"));
        }
        let input = env.read(53)?;
        // FungibleAssetLocation::Here is SCALE discriminant 0 followed by u32.
        if input.first() != Some(&0) {
            return Err(DispatchError::Other("unsupported asset location"));
        }
        let invalid = |_| DispatchError::Other("invalid asset input");
        match function {
            0 => {
                let (asset, who) =
                    <(u32, Account)>::decode_all(&mut &input[1..]).map_err(invalid)?;
                env.write(
                    &<Assets as Inspect<Account>>::balance(asset, &who).encode(),
                    false,
                    None,
                )?;
            }
            1 => {
                let (asset, amount) =
                    <(u32, u128)>::decode_all(&mut &input[1..]).map_err(invalid)?;
                let caller = env.ext().caller().account_id()?.clone();
                let address = env.ext().address().clone();
                match <Assets as Mutate<Account>>::transfer(
                    asset,
                    &caller,
                    &address,
                    amount,
                    Preservation::Preserve,
                ) {
                    Ok(transferred) => env.write(&transferred.encode(), false, None)?,
                    Err(_) => return Ok(RetVal::Converging(0x0001_0000)),
                }
            }
            2 => {
                let (asset, amount, beneficiary) =
                    <(u32, u128, Account)>::decode_all(&mut &input[1..]).map_err(invalid)?;
                let address = env.ext().address().clone();
                match <Assets as Mutate<Account>>::transfer(
                    asset,
                    &address,
                    &beneficiary,
                    amount,
                    Preservation::Preserve,
                ) {
                    Ok(transferred) => env.write(&transferred.encode(), false, None)?,
                    Err(_) => return Ok(RetVal::Converging(0x0001_0001)),
                }
            }
            _ => unreachable!(),
        }
        Ok(RetVal::Converging(0))
    }
}
