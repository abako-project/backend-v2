//! Kreivo for the backend, through sube.
//!
//! sube is single-threaded, so one thread owns the connection and [`Chain`]
//! handles queue their requests to it. The connection string selects the
//! chain: `wss://…` reaches a node (feature `live`) and `fork:<snapshot>` boots
//! an in-process fork of Kreivo (feature `fork`), which runs the same runtime
//! and adds the development operations of `Chain::mock`.
//!
//! Nothing here knows the marketplace: callers bring contract addresses and
//! encoded messages, and get weights, receipts and return data back.
#![allow(
    clippy::missing_errors_doc,
    reason = "Every operation fails with `Error`, whose variants say when."
)]

#[cfg(not(any(feature = "live", feature = "fork")))]
compile_error!("enable the `live` feature, the `fork` feature, or both");

mod contracts;
mod identity;
#[cfg(feature = "fork")]
mod mock;
mod worker;

use std::sync::Arc;
use std::time::Duration;

use libwallet::{Pair, Signer};
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroizing;

pub use contracts::{DryRun, Returned, Weight, message};
pub use identity::{
    DeviceKey, Enrollment, PasskeyAssertion, PasskeyRegistration, SessionRequest, UserId,
    passkey_device,
};
#[cfg(feature = "fork")]
pub use mock::{Mock, Origin, Reader};
pub use protocol::AccountId32;
use worker::{Job, State};

/// Requests waiting for the chain thread. Senders wait when it is full.
const QUEUE: usize = 32;
/// How long a caller waits for its request, queueing included. The chain
/// thread is not interrupted: a request that timed out may still complete.
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

/// Why a chain request failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The node or fork could not be reached or answered with an error.
    #[error("chain unavailable: {0}")]
    Unavailable(String),
    /// The chain thread has stopped.
    #[error("the chain worker stopped")]
    Closed,
    /// No answer within the request timeout; the outcome is unknown.
    #[error("chain request timed out")]
    Timeout,
    /// The request cannot be encoded for this runtime or breaks a local rule.
    #[error("invalid chain request: {0}")]
    Invalid(String),
    /// The chain refused the transaction before including it.
    #[error("transaction rejected: {0}")]
    Rejected(String),
    /// The chain answered with something this crate does not understand.
    #[error("unexpected chain response: {0}")]
    Decode(String),
    /// A development operation was asked of a chain that is not a fork.
    #[error("not a fork")]
    NotAFork,
}

/// Result of a chain request.
pub type Result<T> = core::result::Result<T, Error>;

/// A finalized block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// Its height.
    pub number: u32,
    /// Its hash.
    pub hash: [u8; 32],
}

/// The chain behind a [`Chain`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// Hash of block zero, which every transaction commits to.
    pub genesis: [u8; 32],
    /// Version of the runtime transactions are built for.
    pub spec_version: u32,
    /// The latest finalized block.
    pub head: Block,
    /// How long a pallet-pass session lasts, in blocks of the clock the
    /// runtime gives pallet-pass. On Kreivo that is the relay chain's, six
    /// seconds a block; a fork advances it by one with every block it builds.
    pub max_session_blocks: u32,
    /// Whether this is an in-process fork.
    pub fork: bool,
}

/// One event of a transaction, as the runtime metadata names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// The pallet that deposited it.
    pub pallet: String,
    /// The event variant.
    pub variant: String,
    /// Its fields in sube's text format, when they decode.
    pub fields: Option<String>,
}

/// A transaction the chain finalized, whether or not its dispatch succeeded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Hash of the finalized block that includes it, as `0x` hex.
    pub block: String,
    /// Its position in that block.
    pub index: u32,
    /// The dispatch error, if the call failed after being included. Fees are
    /// paid either way.
    pub failure: Option<String>,
    /// Everything the transaction emitted.
    pub events: Vec<Event>,
}

impl Receipt {
    /// Whether the call was dispatched successfully.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.failure.is_none()
    }
}

/// An sr25519 key this process holds: the operator's, or a session's.
///
/// Clones share the secret, which is wiped when the last one is dropped.
#[derive(Clone)]
pub struct Key {
    pair: Arc<libwallet::any::Pair>,
    account: AccountId32,
}

impl Key {
    /// The key of a 32-byte seed.
    pub fn from_seed(seed: &Zeroizing<[u8; 32]>) -> Result<Self> {
        let unusable = || Error::Invalid("unusable key seed".into());
        let pair = <libwallet::any::Pair as Pair>::from_bytes(seed.as_slice())
            .filter(|pair| matches!(pair, libwallet::any::Pair::Sr25519(_)))
            .ok_or_else(unusable)?;
        let public = <[u8; 32]>::try_from(pair.public().as_ref()).map_err(|_| unusable())?;
        Ok(Self {
            pair: Arc::new(pair),
            account: AccountId32::from_bytes(public),
        })
    }

    /// The account this key signs for.
    #[must_use]
    pub const fn account(&self) -> AccountId32 {
        self.account
    }

    /// Sign `bytes` with this key.
    pub async fn sign(&self, bytes: &[u8]) -> Result<[u8; 64]> {
        let signature = self
            .pair
            .sign_msg(bytes)
            .await
            .map_err(|error| Error::Invalid(format!("the key did not sign: {error}")))?;
        <[u8; 64]>::try_from(signature.as_ref())
            .map_err(|_| Error::Invalid("the key's signature is not 64 bytes".into()))
    }
}

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Key({})", self.account())
    }
}

/// Who authorizes a transaction.
#[derive(Clone, Debug)]
pub enum Authority {
    /// An account signing for itself.
    Key(Key),
    /// A pallet-pass session key acting for the pass account that opened it
    /// with [`Chain::open_session`]. The account pays; the key holds nothing.
    Session {
        /// The pass account the call runs as.
        account: AccountId32,
        /// The session key.
        key: Key,
    },
}

/// A handle to the chain thread. Cheap to clone; the thread stops when the
/// last one is dropped.
#[derive(Clone)]
pub struct Chain {
    jobs: mpsc::Sender<Job>,
}

impl Chain {
    /// Connect to `url` and start the chain thread.
    ///
    /// Fails if the chain cannot be reached, or lacks pallet-pass or
    /// pallet-contracts.
    pub async fn connect(url: &str) -> Result<Self> {
        let (jobs, queue) = mpsc::channel(QUEUE);
        let (ready, connected) = oneshot::channel();
        let url = url.to_owned();
        std::thread::Builder::new()
            .name("kreivo".into())
            .spawn(move || worker::run(&url, queue, ready))
            .map_err(|error| Error::Unavailable(error.to_string()))?;
        connected.await.map_err(|_| Error::Closed)??;
        Ok(Self { jobs })
    }

    /// Run `request` on the chain thread and wait for its answer.
    pub(crate) async fn run<T, F>(&self, request: F) -> Result<T>
    where
        T: Send + 'static,
        F: AsyncFnOnce(&mut State) -> Result<T> + Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        let job: Job = Box::new(move |state| {
            Box::pin(async move {
                // The caller may have timed out and gone.
                let _ = reply.send(request(state).await);
            })
        });
        let queued = async {
            self.jobs.send(job).await.map_err(|_| Error::Closed)?;
            answer.await.map_err(|_| Error::Closed)?
        };
        tokio::time::timeout(REQUEST_TIMEOUT, queued)
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// The chain's identity and latest finalized block.
    pub async fn info(&self) -> Result<Info> {
        self.run(async |state| state.info().await).await
    }

    /// Run a contract message as `caller` without submitting anything.
    ///
    /// The result is what the message would do on the latest finalized block:
    /// the gas and deposit to allow for, and its return data.
    pub async fn dry_run(
        &self,
        caller: AccountId32,
        contract: AccountId32,
        input: Vec<u8>,
    ) -> Result<DryRun> {
        self.run(async move |state| state.dry_run(caller, contract, &input).await)
            .await
    }

    /// Submit a contract message and wait for its block to be finalized.
    ///
    /// `gas` comes from a [`Chain::dry_run`]. A receipt is returned for a failed
    /// dispatch too; an [`Error`] means the transaction was not included, or
    /// that its outcome is unknown.
    pub async fn call(
        &self,
        authority: Authority,
        contract: AccountId32,
        input: Vec<u8>,
        gas: Weight,
    ) -> Result<Receipt> {
        self.run(async move |state| state.call(&authority, contract, &input, gas, None).await)
            .await
    }

    /// Like [`Chain::call`], but the call fails if it would lock more than
    /// `deposit` as storage deposit. Both limits come from a
    /// [`Chain::dry_run`].
    pub async fn call_within(
        &self,
        authority: Authority,
        contract: AccountId32,
        input: Vec<u8>,
        gas: Weight,
        deposit: u128,
    ) -> Result<Receipt> {
        self.run(async move |state| {
            state
                .call(&authority, contract, &input, gas, Some(deposit))
                .await
        })
        .await
    }

    /// Move native balance from `from`, keeping its account alive.
    pub async fn transfer(&self, from: Key, to: AccountId32, amount: u128) -> Result<Receipt> {
        self.run(async move |state| state.transfer(&from, to, amount).await)
            .await
    }

    /// Mint `amount` of the fork's payment asset to `to`, signed by its
    /// issuer. See [`Mock::create_asset`].
    #[cfg(feature = "fork")]
    pub async fn mint(&self, issuer: Key, to: AccountId32, amount: u128) -> Result<Receipt> {
        self.run(async move |state| state.mint(&issuer, to, amount).await)
            .await
    }

    /// Start registering a pass account: what the user's device must sign.
    pub async fn enrollment(&self, user: UserId) -> Result<Enrollment> {
        self.run(async move |state| state.enrollment(user).await)
            .await
    }

    /// Register the pass account of `enrollment` with the device that signed
    /// it. `registrar` pays; the chain verifies the device's signature.
    pub async fn register(
        &self,
        registrar: Key,
        enrollment: Enrollment,
        device: DeviceKey,
    ) -> Result<Receipt> {
        self.run(async move |state| state.register(&registrar, &enrollment, &device).await)
            .await
    }

    /// Register the pass account of `enrollment` like [`Chain::register`] and
    /// give it `funds` of the registrar's balance in the same transaction, so
    /// that no account is left registered and unable to pay.
    pub async fn register_funded(
        &self,
        registrar: Key,
        enrollment: Enrollment,
        device: DeviceKey,
        funds: u128,
    ) -> Result<Receipt> {
        self.run(async move |state| {
            state
                .register_funded(&registrar, &enrollment, &device, funds)
                .await
        })
        .await
    }

    /// Start opening a session for `user`: what their `device` must sign to
    /// let the key of `session` call contracts as the pass account.
    pub async fn session_request(
        &self,
        user: UserId,
        device: AccountId32,
        session: AccountId32,
    ) -> Result<SessionRequest> {
        self.run(async move |state| state.session_request(user, device, session).await)
            .await
    }

    /// Start replacing the device `old` of a pass account with the device
    /// that signed `enrollment`, as when a password changes: what `old` must
    /// sign. Submit it with [`Chain::open_session`], which takes any request a
    /// device authorizes. The new device becomes an admin device and the old
    /// one is removed in the same transaction.
    pub async fn replacement_request(
        &self,
        old: AccountId32,
        enrollment: Enrollment,
        new: DeviceKey,
    ) -> Result<SessionRequest> {
        self.run(async move |state| state.replacement_request(old, &enrollment, &new).await)
            .await
    }

    /// Start adding a passkey to a pass account: what its admin device
    /// `admin` must sign. `enrollment` carried the challenge the passkey was
    /// created with. Submit it with [`Chain::open_session`]. The passkey may
    /// call contracts and open sessions, and nothing else.
    pub async fn passkey_addition_request(
        &self,
        admin: AccountId32,
        enrollment: Enrollment,
        passkey: PasskeyRegistration,
    ) -> Result<SessionRequest> {
        self.run(async move |state| {
            state
                .passkey_addition_request(admin, &enrollment, &passkey)
                .await
        })
        .await
    }

    /// Start removing `device` from `user`'s pass account: what its admin
    /// device `admin` must sign. Submit it with [`Chain::open_session`].
    pub async fn removal_request(
        &self,
        user: UserId,
        admin: AccountId32,
        device: AccountId32,
    ) -> Result<SessionRequest> {
        self.run(async move |state| state.removal_request(user, admin, device).await)
            .await
    }

    /// Start opening a session with the passkey `device`: its
    /// [`SessionRequest::challenge`] is what the passkey must sign.
    pub async fn passkey_session_request(
        &self,
        user: UserId,
        device: AccountId32,
        session: AccountId32,
    ) -> Result<SessionRequest> {
        self.run(async move |state| state.passkey_session_request(user, device, session).await)
            .await
    }

    /// Open the session of `request` with what the passkey answered. The
    /// chain verifies it.
    pub async fn open_passkey_session(
        &self,
        request: SessionRequest,
        assertion: PasskeyAssertion,
    ) -> Result<Receipt> {
        self.run(async move |state| state.open_passkey_session(&request, &assertion).await)
            .await
    }

    /// Open the session of `request` with the device's signature over it.
    ///
    /// The chain verifies the signature: a successful receipt proves the
    /// device's holder asked for this session. The pass account pays the fee.
    /// The session then lasts [`Info::max_session_blocks`]; once it has
    /// expired the chain rejects what its key signs.
    pub async fn open_session(
        &self,
        request: SessionRequest,
        signature: [u8; 64],
    ) -> Result<Receipt> {
        self.run(async move |state| state.open_session(&request, signature).await)
            .await
    }
}

impl core::fmt::Debug for Chain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Chain").finish_non_exhaustive()
    }
}

impl From<sube::Error> for Error {
    fn from(error: sube::Error) -> Self {
        match error {
            sube::Error::BadInput
            | sube::Error::Encode(_)
            | sube::Error::Mapping(_)
            | sube::Error::CallNotFound
            | sube::Error::PalletNotFound(_)
            | sube::Error::MissingExtensionValue(_)
            | sube::Error::Signing(_) => Self::Invalid(error.to_string()),
            sube::Error::Decode(_) | sube::Error::BadMetadata => Self::Decode(error.to_string()),
            sube::Error::OperationFailed(reason) if reason.starts_with("tx invalid") => {
                Self::Rejected(reason)
            }
            _ => Self::Unavailable(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use libwallet::Signer;

    use super::{Key, Zeroizing};

    type Outcome = core::result::Result<(), Box<dyn std::error::Error>>;

    /// The accounts of two seeds, as every release of this crate derived them.
    /// A stored device key or operator account depends on them.
    #[test]
    fn a_seed_gives_the_same_account() -> Outcome {
        let repeated = Key::from_seed(&Zeroizing::new([7; 32]))?;
        assert_eq!(
            repeated.account().into_bytes(),
            [
                124, 15, 70, 157, 59, 211, 64, 186, 231, 24, 32, 63, 163, 12, 160, 113, 165, 227,
                124, 117, 30, 137, 29, 189, 237, 131, 123, 33, 61, 69, 217, 29
            ]
        );
        let mut counting = Zeroizing::new([0; 32]);
        for (byte, value) in counting.iter_mut().zip(0..) {
            *byte = value;
        }
        assert_eq!(
            Key::from_seed(&counting)?.account().into_bytes(),
            [
                226, 17, 23, 121, 152, 22, 24, 112, 94, 202, 206, 161, 175, 111, 249, 53, 11, 206,
                43, 45, 204, 208, 62, 12, 62, 1, 235, 12, 130, 61, 38, 102
            ]
        );
        Ok(())
    }

    #[test]
    fn a_signature_verifies_for_its_message_only() -> Outcome {
        let key = Key::from_seed(&Zeroizing::new([7; 32]))?;
        let signature = smol::block_on(key.sign(b"a message"))?;
        assert!(smol::block_on(key.pair.verify(b"a message", signature)));
        assert!(!smol::block_on(
            key.pair.verify(b"another message", signature)
        ));
        Ok(())
    }
}
