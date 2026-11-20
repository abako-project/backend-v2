//! Development operations of an in-process fork.
//!
//! They change the fork's state directly, as no account could on Kreivo:
//! balances appear, and contracts are deployed through the runtime API, which
//! checks neither Kreivo's deployment origins nor its block limits. Each one
//! then builds a block, so that the head's hash names the new state.

use std::path::PathBuf;

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

    fn save(&self, path: &std::path::Path) -> Result<()> {
        let fork = self.fork()?;
        let snapshot = fork.borrow().snapshot();
        // Write beside the target and rename, so a crash leaves the old file.
        let partial = path.with_extension("partial");
        snapshot.save(&partial)?;
        std::fs::rename(&partial, path).map_err(|error| Error::Unavailable(error.to_string()))
    }
}
