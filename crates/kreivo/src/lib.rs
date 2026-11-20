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

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroizing;

pub use contracts::{DryRun, Returned, Weight, message};
pub use identity::{DeviceKey, Enrollment, SessionRequest, UserId};
#[cfg(feature = "fork")]
pub use mock::Mock;
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
#[derive(Clone)]
pub struct Key(subxt_signer::sr25519::Keypair);

impl Key {
    /// The key of a 32-byte seed.
    pub fn from_seed(seed: &Zeroizing<[u8; 32]>) -> Result<Self> {
        subxt_signer::sr25519::Keypair::from_secret_key(**seed)
            .map(Self)
            .map_err(|_| Error::Invalid("unusable key seed".into()))
    }

    /// The account this key signs for.
    #[must_use]
    pub fn account(&self) -> AccountId32 {
        AccountId32::from_bytes(self.0.public_key().0)
    }

    pub(crate) fn sign(&self, bytes: &[u8]) -> [u8; 64] {
        self.0.sign(bytes).0
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
        self.run(async move |state| state.call(&authority, contract, &input, gas).await)
            .await
    }

    /// Move native balance from `from`, keeping its account alive.
    pub async fn transfer(&self, from: Key, to: AccountId32, amount: u128) -> Result<Receipt> {
        self.run(async move |state| state.transfer(&from, to, amount).await)
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
            | sube::Error::MissingExtensionValue(_) => Self::Invalid(error.to_string()),
            sube::Error::Decode(_) | sube::Error::BadMetadata => Self::Decode(error.to_string()),
            sube::Error::OperationFailed(reason) if reason.contains("invalid") => {
                Self::Rejected(reason)
            }
            _ => Self::Unavailable(error.to_string()),
        }
    }
}
