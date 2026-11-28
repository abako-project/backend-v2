//! Development operations of an in-process fork.
//!
//! They change the fork's state directly, as no account could on Kreivo:
//! balances appear, and contracts are deployed through the runtime API, which
//! checks neither Kreivo's deployment origins nor its block limits. Each one
//! then builds a block, so that the head's hash names the new state.

use std::path::PathBuf;

use blake2::{
    Blake2b, Digest,
    digest::consts::{U16, U32},
};
use parity_scale_codec::{Decode, Encode};
use protocol::AccountId32;
use sube::fork::{Diff, SharedFork};
use sube::metadata::StorageKey;
use sube::{DynValue, scales};

use crate::contracts::{ExecReturnValue, decode_result, hex0x};
use crate::{Chain, Error, Result, Weight, worker::State};

/// Bit of `ReturnFlags` an ink! constructor sets when it reverts.
const REVERT: u32 = 1;
/// `ExtraFlags` of an account created since balances dropped reserved-balance
/// accounting from the reference counters.
const NEW_LOGIC: u128 = 1 << 127;

/// The development operations of a [`Chain`] that is a fork.
#[derive(Debug)]
pub struct Mock<'a>(&'a Chain);

impl Chain {
    /// Development operations. They fail with [`Error::NotAFork`] on a node.
    #[must_use]
    pub const fn mock(&self) -> Mock<'_> {
        Mock(self)
    }
}

impl Mock<'_> {
    /// Create `account` holding `free` native balance. An account that
    /// already exists is refused, since its counters would be overwritten:
    /// transfer to it instead.
    pub async fn endow(&self, account: AccountId32, free: u128) -> Result<()> {
        self.0
            .run(async move |state| state.endow(account, free))
            .await
    }

    /// Upload contract `code` as `owner`, who pays its deposit. Returns the
    /// code hash.
    pub async fn upload(&self, owner: AccountId32, code: Vec<u8>) -> Result<[u8; 32]> {
        self.0
            .run(async move |state| state.upload(owner, &code))
            .await
    }

    /// Instantiate `code` as `deployer` with the constructor `input`, within
    /// `gas`, which may exceed a block. Returns the contract's address.
    pub async fn deploy(
        &self,
        deployer: AccountId32,
        code: Vec<u8>,
        input: Vec<u8>,
        salt: Vec<u8>,
        gas: Weight,
    ) -> Result<AccountId32> {
        self.0
            .run(async move |state| state.deploy(deployer, &code, &input, &salt, gas))
            .await
    }

    /// Build a block that lands `relay_blocks` relay chain blocks, six seconds
    /// each, after the head: Kreivo's clock for timestamps and sessions.
    ///
    /// The runtime's scheduler then serves the skipped relay blocks a few dozen
    /// per block, refusing transactions meanwhile, so more blocks are built
    /// until it has caught up: the head lands a little later than asked.
    pub async fn advance(&self, relay_blocks: u32) -> Result<()> {
        self.0
            .run(async move |state| state.advance(relay_blocks))
            .await
    }

    /// Save the fork's head and state to `path`, to boot it again with
    /// `fork:<path>`.
    pub async fn save(&self, path: PathBuf) -> Result<()> {
        self.0.run(async move |state| state.save(&path)).await
    }

    /// Create the payment asset the pinned contracts name, `Here(1)`, owned
    /// and issued by `owner`. Kreivo creates assets through governance; a fork
    /// writes the asset's record. The owner then mints with [`Chain::mint`].
    pub async fn create_asset(&self, owner: AccountId32) -> Result<()> {
        self.0
            .run(async move |state| state.create_asset(owner))
            .await
    }

    /// Let the session of `key` outlive its relay blocks: the removal the
    /// chain scheduled for it is cancelled, as Root. A fork's clock moves with
    /// every block it builds, so a busy fork would end sessions long before
    /// their time; whoever calls this ends them with [`Mock::end_session`].
    pub async fn keep_session(&self, key: AccountId32) -> Result<()> {
        self.0
            .run(async move |state| {
                // The name pallet-pass gives the removal it schedules.
                let name: [u8; 32] =
                    Blake2b::<U32>::digest(("remove_session_key", key.as_bytes()).encode()).into();
                let call = state.sube.prepare_call(
                    "scheduler/cancel_named",
                    &DynValue::obj(&[("id", DynValue::from(name))]),
                )?;
                state.root(&call.bytes)
            })
            .await
    }

    /// End the session of `key` now, as Root, freeing its place among the
    /// account's sessions.
    pub async fn end_session(&self, key: AccountId32) -> Result<()> {
        self.0
            .run(async move |state| {
                let call = state.sube.prepare_call(
                    "pass/remove_session_key",
                    &DynValue::obj(&[("session_key", DynValue::from(key.into_bytes()))]),
                )?;
                state.root(&call.bytes)
            })
            .await
    }

    /// Whether `device` is a device of the pass account `account` now.
    pub async fn has_device(&self, account: AccountId32, device: AccountId32) -> Result<bool> {
        self.0
            .run(async move |state| {
                let fork = state.fork()?;
                let metadata = state.sube.metadata();
                let pass = metadata
                    .pallet_by_name(&state.pass.pallet)
                    .ok_or_else(|| Error::Decode("no Pass pallet".into()))?;
                let key = StorageKey::build_with_registry(
                    &metadata.registry,
                    pass,
                    "Devices",
                    &[hex0x(account.as_bytes()), hex0x(device.as_bytes())],
                )?
                .key();
                let fork = fork.borrow();
                Ok(fork.storage(fork.head_index(), None, &key).is_some())
            })
            .await
    }

    /// Dispatch a call with `origin` and no signature, as no account could on
    /// Kreivo: `call` is `pallet/call` and `arguments` its fields in sube's
    /// text form, `(name:value;...)`. For what only governance or a
    /// community's own account does on the chain.
    pub async fn dispatch(&self, origin: Origin, call: String, arguments: String) -> Result<()> {
        self.0
            .run(async move |state| {
                let call = state.sube.prepare_call(&call, &sube::Text(&arguments))?;
                state.dispatch_as(&origin, &call.bytes)
            })
            .await
    }

    /// The bytes of the storage value `item` of `pallet` at the head, if it
    /// is set.
    pub async fn storage_value(&self, pallet: String, item: String) -> Result<Option<Vec<u8>>> {
        self.0
            .run(async move |state| {
                let fork = state.fork()?;
                let metadata = state.sube.metadata();
                let entry = metadata
                    .pallet_by_name(&pallet)
                    .ok_or_else(|| Error::Decode(format!("no {pallet} pallet")))?;
                let key =
                    StorageKey::build_with_registry::<&str>(&metadata.registry, entry, &item, &[])?
                        .key();
                let fork = fork.borrow();
                Ok(fork.storage(fork.head_index(), None, &key))
            })
            .await
    }

    /// The native balance `account` can spend.
    pub async fn free_balance(&self, account: AccountId32) -> Result<u128> {
        self.0
            .run(async move |state| {
                let fork = state.fork()?;
                let metadata = state.sube.metadata();
                let system = metadata
                    .pallet_by_name("System")
                    .ok_or_else(|| Error::Decode("no System pallet".into()))?;
                let key = StorageKey::build_with_registry(
                    &metadata.registry,
                    system,
                    "Account",
                    &[hex0x(account.as_bytes())],
                )?
                .key();
                let fork = fork.borrow();
                let Some(info) = fork.storage(fork.head_index(), None, &key) else {
                    return Ok(0);
                };
                // AccountInfo { nonce: u32, consumers, providers, sufficients: u32, data: { free, .. } }
                let free = info
                    .get(16..32)
                    .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
                    .ok_or_else(|| Error::Decode("AccountInfo is too short".into()))?;
                Ok(u128::from_le_bytes(free))
            })
            .await
    }

    /// Run `reads` on the chain thread against the fork's head. Nothing else
    /// runs meanwhile, so what it reads is one state.
    pub async fn read<T, F>(&self, reads: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Reader<'_>) -> T + Send + 'static,
    {
        self.0
            .run(async move |state| {
                let mut reader = Reader {
                    fork: state.fork()?,
                    state,
                    tries: std::collections::BTreeMap::new(),
                };
                Ok(reads(&mut reader))
            })
            .await
    }

    /// Bring the fork's clock to `unix_ms`, the wall clock: the next blocks
    /// land there. Does nothing when the fork is not behind by a relay block.
    pub async fn follow_clock(&self, unix_ms: u64) -> Result<()> {
        self.0
            .run(async move |state| {
                loop {
                    let behind = unix_ms.saturating_sub(state.timestamp()?) / RELAY_BLOCK_MS;
                    let behind = u32::try_from(behind)
                        .map_err(|_| Error::Invalid("the fork is too far behind".into()))?;
                    match behind {
                        0 => return Ok(()),
                        // The scheduler serves a short gap within the block.
                        1..=SERVED_PER_BLOCK => {
                            let fork = state.fork()?;
                            fork.borrow_mut().time_travel(behind);
                            return State::seal(&fork);
                        }
                        // A long one takes more blocks, of a relay block each:
                        // jump short of the target by what they will add.
                        _ => state.advance((behind - 8) / 9 * 8)?,
                    }
                }
            })
            .await
    }
}

/// The asset the pinned contracts pay in, as the runtime names it.
pub(crate) const PAYMENT_ASSET: &str = "FungibleAssetLocation::Here(1)";

/// A relay chain block, Kreivo's clock.
const RELAY_BLOCK_MS: u64 = 6_000;
/// Skipped relay blocks whose agendas the scheduler serves in one block.
const SERVED_PER_BLOCK: u32 = 24;

/// The pallet's error a dry-run's dispatch failed with, as `Pallet::Error`.
fn dispatch_failure(metadata: &sube::Metadata, output: &[u8]) -> Option<String> {
    type Weight = (
        parity_scale_codec::Compact<u64>,
        parity_scale_codec::Compact<u64>,
    );
    /// `DispatchError::Module`.
    const MODULE: u8 = 3;
    // Ok(CallDryRunEffects { execution_result: Err(DispatchErrorWithPostInfo {
    //     post_info: { actual_weight: Option<Weight>, pays_fee }, error }), .. })
    let mut input = output.strip_prefix(&[0, 1])?;
    Option::<Weight>::decode(&mut input).ok()?;
    let (_pays, variant, pallet, error) = <(u8, u8, u8, [u8; 4])>::decode(&mut input).ok()?;
    if variant != MODULE {
        return None;
    }
    let pallet = metadata
        .pallets
        .iter()
        .find(|entry| entry.index == pallet)?;
    let sube::scales::TypeDef::Variant(errors) = metadata.registry.resolve(pallet.errors_ty?)?
    else {
        return None;
    };
    let error = errors.variant(error[0]).ok()?;
    Some(format!("{}::{}", pallet.name, error.name()))
}

/// Who a call of [`Mock::dispatch`] comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The chain's root.
    Root,
    /// This account, without its signature.
    Signed(AccountId32),
}

/// Reads of the fork's head, for [`Mock::read`].
pub struct Reader<'a> {
    state: &'a State,
    fork: SharedFork,
    /// Child trie of each contract read so far.
    tries: std::collections::BTreeMap<AccountId32, Vec<u8>>,
}

impl core::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reader").finish_non_exhaustive()
    }
}

impl Reader<'_> {
    /// The value a contract stores under `key`, as its `get_storage` reads it.
    pub fn contract_storage(
        &mut self,
        contract: AccountId32,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let trie = if let Some(trie) = self.tries.get(&contract) {
            trie.clone()
        } else {
            let trie = self.state.contract_trie(&self.fork, contract)?;
            self.tries.insert(contract, trie.clone());
            trie
        };
        // pallet-contracts hashes a variable-length key with Blake2_128Concat.
        let hash: [u8; 16] = Blake2b::<U16>::digest(key).into();
        let hashed = [&hash[..], key].concat();
        let fork = self.fork.borrow();
        Ok(fork.storage(fork.head_index(), Some(&trie), &hashed))
    }

    /// Run a contract message as `caller` on the head, changing nothing.
    pub fn query(
        &mut self,
        caller: AccountId32,
        contract: AccountId32,
        input: &[u8],
    ) -> Result<crate::Returned> {
        let parameters = crate::contracts::call_parameters(caller, contract, input);
        let head = self.fork.borrow().head_index();
        let output = self
            .fork
            .borrow_mut()
            .call(head, "ContractsApi_call", &parameters)?;
        let (_, _, outcome) = decode_result(&output, |input| ExecReturnValue::decode(input))?;
        let value = outcome.map_err(Error::Rejected)?;
        Ok(crate::Returned {
            reverted: value.flags & REVERT != 0,
            data: value.data,
        })
    }

    /// The head's time, in milliseconds since the Unix epoch.
    pub fn timestamp(&self) -> Result<u64> {
        self.state.timestamp()
    }
}

#[derive(Decode)]
struct Instantiated {
    result: ExecReturnValue,
    account: [u8; 32],
}

impl From<sube::fork::Error> for Error {
    fn from(error: sube::fork::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

impl State {
    fn fork(&self) -> Result<SharedFork> {
        self.sube.fork().cloned().ok_or(Error::NotAFork)
    }

    fn seal(fork: &SharedFork) -> Result<()> {
        fork.borrow_mut().new_block()?;
        Ok(())
    }

    fn advance(&self, relay_blocks: u32) -> Result<()> {
        let fork = self.fork()?;
        // The next block lands that many relay blocks after its parent, and
        // never fewer than one.
        fork.borrow_mut().time_travel(relay_blocks);
        Self::seal(&fork)?;

        // The scheduler serves the agendas of the skipped relay blocks over
        // the next blocks and fills them while it does: transactions are
        // refused until it is done. Build those blocks now.
        let metadata = self.sube.metadata();
        let Some(behind) = metadata.pallet_by_name("Scheduler").and_then(|scheduler| {
            let no_keys: &[&str] = &[];
            StorageKey::build_with_registry(
                &metadata.registry,
                scheduler,
                "IncompleteSince",
                no_keys,
            )
            .ok()
        }) else {
            return Ok(());
        };
        let behind = behind.key();
        for _ in 0..relay_blocks / 8 + 8 {
            let caught_up = {
                let fork = fork.borrow();
                fork.storage(fork.head_index(), None, &behind).is_none()
            };
            if caught_up {
                break;
            }
            Self::seal(&fork)?;
        }
        Ok(())
    }

    fn endow(&self, account: AccountId32, free: u128) -> Result<()> {
        let fork = self.fork()?;
        let metadata = self.sube.metadata();
        let system = metadata
            .pallet_by_name("System")
            .ok_or_else(|| Error::Decode("no System pallet".into()))?;
        let entry = StorageKey::build_with_registry(
            &metadata.registry,
            system,
            "Account",
            &[hex0x(account.as_bytes())],
        )?;
        let key = entry.key();
        {
            let fork = fork.borrow();
            if fork.storage(fork.head_index(), None, &key).is_some() {
                return Err(Error::Invalid(format!("{account} already exists")));
            }
        }
        let info = DynValue::obj(&[
            ("nonce", DynValue::U32(0)),
            ("consumers", DynValue::U32(0)),
            ("providers", DynValue::U32(1)),
            ("sufficients", DynValue::U32(0)),
            (
                "data",
                DynValue::obj(&[
                    ("free", DynValue::U128(free)),
                    ("reserved", DynValue::U128(0)),
                    ("frozen", DynValue::U128(0)),
                    ("flags", DynValue::U128(NEW_LOGIC)),
                ]),
            ),
        ]);
        let value = scales::to_vec_with_info(&info, Some((&metadata.registry, entry.ty)))
            .map_err(|error| Error::Invalid(format!("AccountInfo: {error}")))?;
        let mut diff = Diff::default();
        diff.insert(key, value);

        // The new balance is part of the issuance: the runtime refuses to move
        // more than the issuance it knows of.
        let balances = metadata
            .pallet_by_name("Balances")
            .ok_or_else(|| Error::Decode("no Balances pallet".into()))?;
        let no_keys: &[&str] = &[];
        let issuance = StorageKey::build_with_registry(
            &metadata.registry,
            balances,
            "TotalIssuance",
            no_keys,
        )?
        .key();
        let issued = {
            let fork = fork.borrow();
            match fork.storage(fork.head_index(), None, &issuance) {
                Some(bytes) => u128::decode(&mut &bytes[..])
                    .map_err(|error| Error::Decode(format!("TotalIssuance: {error}")))?,
                None => 0,
            }
        };
        let issued = issued
            .checked_add(free)
            .ok_or_else(|| Error::Invalid("the issuance would overflow".into()))?;
        diff.insert(issuance, issued.encode());
        fork.borrow_mut().set_storage(diff)?;
        Self::seal(&fork)
    }

    fn upload(&self, owner: AccountId32, code: &[u8]) -> Result<[u8; 32]> {
        const ENFORCED: u8 = 0;
        let fork = self.fork()?;
        // ContractsApi::upload_code(origin, code, storage_deposit_limit, determinism)
        let parameters = (owner.as_bytes(), code, None::<u128>, ENFORCED).encode();
        // Try it first: only an accepted upload gets its changes kept.
        let head = fork.borrow().head_index();
        let output = fork
            .borrow_mut()
            .call(head, "ContractsApi_upload_code", &parameters)?;
        // Result<CodeUploadReturnValue { code_hash, deposit }, DispatchError>
        let uploaded = match output.split_first() {
            Some((0, mut value)) => <([u8; 32], u128)>::decode(&mut value)
                .map_err(|error| Error::Decode(error.to_string()))?,
            _ => return Err(Error::Rejected(format!("upload: {}", hex0x(&output)))),
        };
        fork.borrow_mut()
            .call_mut("ContractsApi_upload_code", &parameters)?;
        Self::seal(&fork)?;
        Ok(uploaded.0)
    }

    fn deploy(
        &self,
        deployer: AccountId32,
        code: &[u8],
        input: &[u8],
        salt: &[u8],
        gas: Weight,
    ) -> Result<AccountId32> {
        const UPLOAD: u8 = 0;
        let fork = self.fork()?;
        // ContractsApi::instantiate(origin, value, gas_limit, storage_deposit_limit, code, data, salt)
        let parameters = (
            deployer.as_bytes(),
            0_u128,
            Some(gas),
            None::<u128>,
            (UPLOAD, code),
            input,
            salt,
        )
            .encode();
        // Try it first: only a constructor that succeeds gets its changes kept.
        let head = fork.borrow().head_index();
        let output = fork
            .borrow_mut()
            .call(head, "ContractsApi_instantiate", &parameters)?;
        let (_, _, outcome) = decode_result(&output, |input| Instantiated::decode(input))?;
        let created = outcome.map_err(|error| Error::Rejected(format!("instantiate: {error}")))?;
        if created.result.flags & REVERT != 0 {
            return Err(Error::Rejected(format!(
                "constructor reverted: {}",
                hex0x(&created.result.data)
            )));
        }
        fork.borrow_mut()
            .call_mut("ContractsApi_instantiate", &parameters)?;
        Self::seal(&fork)?;
        Ok(AccountId32::from_bytes(created.account))
    }

    /// Dispatch `call` as Root, through the runtime's dry-run API, and keep
    /// what it changed. No account can do this on Kreivo.
    fn root(&self, call: &[u8]) -> Result<()> {
        self.dispatch_as(&Origin::Root, call)
    }

    /// Dispatch `call` with `origin`, through the runtime's dry-run API, and
    /// keep what it changed, without any signature.
    fn dispatch_as(&self, origin: &Origin, call: &[u8]) -> Result<()> {
        /// The XCM version of the messages the dry-run would report.
        const XCM_VERSION: u32 = 5;
        // `OriginCaller::system(RawOrigin::Root | Signed(account))`.
        let origin = match origin {
            Origin::Root => vec![0, 0],
            Origin::Signed(account) => [&[0, 1][..], account.as_bytes()].concat(),
        };
        let fork = self.fork()?;
        // DryRunApi::dry_run_call(origin, call, result_xcms_version)
        let parameters = [&origin[..], call, &XCM_VERSION.encode()].concat();
        let head = fork.borrow().head_index();
        let output = fork
            .borrow_mut()
            .call(head, "DryRunApi_dry_run_call", &parameters)?;
        // Result<CallDryRunEffects { execution_result: Result<..>, .. }, Error>
        if !matches!(output.as_slice(), [0, 0, ..]) {
            let reason = dispatch_failure(self.sube.metadata(), &output)
                .unwrap_or_else(|| hex0x(output.get(..output.len().min(48)).unwrap_or_default()));
            return Err(Error::Rejected(format!("privileged call: {reason}")));
        }
        fork.borrow_mut()
            .call_mut("DryRunApi_dry_run_call", &parameters)?;
        Self::seal(&fork)
    }

    fn create_asset(&self, owner: AccountId32) -> Result<()> {
        let fork = self.fork()?;
        let metadata = self.sube.metadata();
        let assets = metadata
            .pallet_by_name("Assets")
            .ok_or_else(|| Error::Decode("no Assets pallet".into()))?;
        let entry =
            StorageKey::build_with_registry(&metadata.registry, assets, "Asset", &[PAYMENT_ASSET])?;
        let key = entry.key();
        {
            let fork = fork.borrow();
            if fork.storage(fork.head_index(), None, &key).is_some() {
                return Err(Error::Invalid("the payment asset already exists".into()));
            }
        }
        let owner = hex0x(owner.as_bytes());
        let details = format!(
            "(owner:{owner};issuer:{owner};admin:{owner};freezer:{owner};supply:0;deposit:0;\
             min_balance:1;is_sufficient:true;accounts:0;sufficients:0;approvals:0;\
             status:AssetStatus::Live)"
        );
        let value = scales::from_text(&details, &metadata.registry, entry.ty)
            .map_err(|error| Error::Invalid(format!("AssetDetails: {error}")))?;
        let mut diff = Diff::default();
        diff.insert(key, value);
        fork.borrow_mut().set_storage(diff)?;
        Self::seal(&fork)
    }

    /// The head's `Timestamp.Now`.
    fn timestamp(&self) -> Result<u64> {
        let fork = self.fork()?;
        let metadata = self.sube.metadata();
        let timestamp = metadata
            .pallet_by_name("Timestamp")
            .ok_or_else(|| Error::Decode("no Timestamp pallet".into()))?;
        let no_keys: &[&str] = &[];
        let key =
            StorageKey::build_with_registry(&metadata.registry, timestamp, "Now", no_keys)?.key();
        let fork = fork.borrow();
        let bytes = fork
            .storage(fork.head_index(), None, &key)
            .ok_or_else(|| Error::Decode("the head has no timestamp".into()))?;
        u64::decode(&mut &bytes[..])
            .map_err(|error| Error::Decode(format!("Timestamp.Now: {error}")))
    }

    /// The child trie that holds `contract`'s storage.
    fn contract_trie(&self, fork: &SharedFork, contract: AccountId32) -> Result<Vec<u8>> {
        let metadata = self.sube.metadata();
        let contracts = metadata
            .pallet_by_name("Contracts")
            .ok_or_else(|| Error::Decode("no Contracts pallet".into()))?;
        let key = StorageKey::build_with_registry(
            &metadata.registry,
            contracts,
            "ContractInfoOf",
            &[hex0x(contract.as_bytes())],
        )?
        .key();
        let fork = fork.borrow();
        let info = fork
            .storage(fork.head_index(), None, &key)
            .ok_or_else(|| Error::Invalid(format!("{contract} is not a contract")))?;
        // ContractInfo starts with its `trie_id`.
        Vec::<u8>::decode(&mut &info[..])
            .map_err(|error| Error::Decode(format!("ContractInfo: {error}")))
    }

    fn save(&self, path: &std::path::Path) -> Result<()> {
        let fork = self.fork()?;
        let snapshot = fork.borrow().snapshot();
        // Write beside the target and rename, so a crash leaves the old file.
        let partial = path.with_extension("partial");
        snapshot.save(&partial)?;
        std::fs::rename(&partial, path).map_err(|error| Error::Unavailable(error.to_string()))
    }
}
