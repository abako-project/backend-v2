//! Embedded ink! v5 runtime and local contract execution.
#![allow(deprecated)] // pallet-contracts retains the pre-v6 Randomness interface.

use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstBool, ConstU32, ConstU128, Nothing, Randomness},
    weights::Weight,
};
use sp_runtime::{AccountId32, BuildStorage, traits::IdentityLookup};

pub mod assets;
pub mod bundle;
pub mod checkpoint;
mod provider;
pub mod transport;
pub mod worker;

#[derive(Debug)]
pub enum ProofError {
    Genesis(String),
    OverlayCommit(String),
    Decode(parity_scale_codec::Error),
    CheckpointVersion(u32),
    InvalidTrieNode,
    Limit,
    Identity,
    Bundle,
}
impl std::fmt::Display for ProofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Genesis(reason) => write!(f, "runtime genesis: {reason}"),
            Self::OverlayCommit(reason) => write!(f, "runtime overlay commit: {reason}"),
            Self::Decode(reason) => write!(f, "checkpoint decode: {reason}"),
            Self::CheckpointVersion(version) => {
                write!(f, "unsupported proof checkpoint: {version}")
            }
            Self::InvalidTrieNode => f.write_str("invalid checkpoint trie node"),
            Self::Limit => f.write_str("sandbox resource limit"),
            Self::Identity => f.write_str("incompatible runtime or bundle"),
            Self::Bundle => f.write_str("invalid pinned contract bundle"),
        }
    }
}
impl std::error::Error for ProofError {}

pub type Account = AccountId32;
pub const GAS: Weight = Weight::from_parts(1_000_000_000_000, 16 * 1024 * 1024);
/// Whole-provider candidate bound; fixture execution retains its original budget.
pub const EXECUTION_GAS: Weight = Weight::from_parts(2_000_000_000_000, 128 * 1024 * 1024);
pub const TECHNICAL_BALANCE: u128 = 1_000_000_000_000_000;

frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system,
        Balances: pallet_balances,
        Timestamp: pallet_timestamp,
        Assets: pallet_assets,
        Contracts: pallet_contracts,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Runtime {
    type Block = frame_system::mocking::MockBlockU32<Runtime>;
    type AccountId = Account;
    type Lookup = IdentityLookup<Account>;
    type AccountData = pallet_balances::AccountData<u128>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Runtime {
    type Balance = u128;
    type AccountStore = System;
    type ExistentialDeposit = ConstU128<1>;
}

#[derive_impl(pallet_timestamp::config_preludes::TestDefaultConfig)]
impl pallet_timestamp::Config for Runtime {
    type Moment = u64;
}

#[derive_impl(pallet_assets::config_preludes::TestDefaultConfig)]
impl pallet_assets::Config for Runtime {
    type Balance = u128;
    type Currency = Balances;
    type CreateOrigin =
        frame_support::traits::AsEnsureOriginWithArg<frame_system::EnsureSigned<Account>>;
    type ForceOrigin = frame_system::EnsureRoot<Account>;
}

parameter_types! {
    pub Schedule: pallet_contracts::Schedule<Runtime> = pallet_contracts::Schedule {
        limits: pallet_contracts::Limits {
            runtime_memory: 748 * 1024 * 1024,
            validator_runtime_memory: 1024 * 1024 * 1024,
            ..Default::default()
        },
        ..Default::default()
    };
    pub const DepositPerByte: u128 = 1;
    pub const DepositPerItem: u128 = 1;
    pub const DefaultDepositLimit: u128 = TECHNICAL_BALANCE;
}

// The v5 contracts do not use seal_random. Do not pretend a fixed hash is random.
pub struct NoRandomness;
impl Randomness<sp_core::H256, u32> for NoRandomness {
    fn random(_: &[u8]) -> (sp_core::H256, u32) {
        (Default::default(), 0)
    }
}

#[derive_impl(pallet_contracts::config_preludes::TestDefaultConfig)]
impl pallet_contracts::Config for Runtime {
    type Time = Timestamp;
    type Randomness = NoRandomness;
    type Currency = Balances;
    type CallFilter = Nothing;
    type CallStack = [pallet_contracts::Frame<Self>; 23];
    type ChainExtension = assets::KreivoAssets;
    type Schedule = Schedule;
    type DepositPerByte = DepositPerByte;
    type DepositPerItem = DepositPerItem;
    type DefaultDepositLimit = DefaultDepositLimit;
    type MaxCodeLen = ConstU32<{ 192 * 1024 }>;
    type UnsafeUnstableInterface = ConstBool<false>;
}

pub fn new_externalities(accounts: Vec<Account>) -> Result<sp_io::TestExternalities, ProofError> {
    let mut storage = frame_system::GenesisConfig::<Runtime>::default()
        .build_storage()
        .map_err(ProofError::Genesis)?;
    pallet_balances::GenesisConfig::<Runtime> {
        balances: accounts
            .into_iter()
            .map(|a| (a, TECHNICAL_BALANCE))
            .collect(),
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .map_err(ProofError::Genesis)?;
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        Timestamp::set_timestamp(1_700_000_000_000);
    });
    Ok(ext)
}
