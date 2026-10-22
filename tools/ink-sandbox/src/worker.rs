//! Private candidate execution, isolated from the default application binary.
//!
//! Raw fixtures supply opaque envelopes; signed fixtures use the real shell.
//! Neither this module nor callers may infer non-execution from a lost response.
use crate::{
    Account, Contracts, GAS, ProofError, Timestamp,
    bundle::{Bundles, Identity},
    checkpoint::Checkpoint,
    new_externalities,
};
use frame_support::weights::Weight;
use generated_contracts::{AccountId32, OperationReceipt, SignedContractCallV1, UnixSeconds};
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism};
use parity_scale_codec::{Decode, Encode};
use provider_execution::Shell;
use sp_core::H256;
use sp_runtime::DispatchError;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};
use tokio::sync::oneshot;
mod archive;
pub mod comparison;
mod directory;
mod evidence_wire;
mod migration;
mod whole;
pub use migration::WholeConfiguration;
pub use whole::{Authority, WholeDiagnostics};
mod projects;
mod signed;
pub use directory::{
    DirectoryFixture, DirectoryProjection, FinancialProjection, MarketplaceProjection,
    MoneyProjection,
};
pub use signed::{CalendarFixture, FixtureWorker};

#[derive(Clone, Copy)]
enum Mode {
    Raw,
    Calendar {
        root: AccountId32,
        gas: Weight,
    },
    Directory {
        root: AccountId32,
        gas: Weight,
        configuration: H256,
        projects: bool,
        money: bool,
        whole: bool,
        authority: Option<Authority>,
    },
}
impl Mode {
    fn key(self) -> &'static str {
        match self {
            Self::Raw => "raw-fixture-v1",
            Self::Calendar { .. } => "signed-calendar-v1",
            Self::Directory { .. } => "signed-directory-v1",
        }
    }
}
enum InitialState {
    Raw {
        accounts: Vec<Account>,
        shell: Vec<u8>,
    },
    Calendar(CalendarFixture),
    Directory(DirectoryFixture),
    Import {
        archive: Box<provider_execution::migration::ArchiveV1>,
        configuration: WholeConfiguration,
    },
    Reopen(WholeConfiguration),
    WholeImport {
        archive: Box<provider_execution::migration::ArchiveV1>,
        configuration: WholeConfiguration,
        authority: Authority,
    },
    WholeReopen {
        configuration: WholeConfiguration,
        authority: Authority,
    },
}
impl InitialState {
    fn mode(&self) -> Mode {
        match self {
            Self::WholeImport {
                configuration,
                authority,
                ..
            }
            | Self::WholeReopen {
                configuration,
                authority,
            } => whole::mode(configuration, *authority),
            Self::Raw { .. } => Mode::Raw,
            Self::Calendar(fixture) => Mode::Calendar {
                root: fixture.root,
                gas: fixture.gas_limit,
            },
            Self::Directory(fixture) => fixture.mode(),
            Self::Import { configuration, .. } | Self::Reopen(configuration) => {
                configuration.mode()
            }
        }
    }
}

pub const QUEUE_CAPACITY: usize = 8;
// Retain the existing unsigned limit plus signature/ABI framing overhead.
pub const MAX_INPUT: usize = generated_contracts::MAX_SIGNABLE_BYTES + 1024;
pub const MAX_SHELL: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Runtime(ProofError),
    Storage(sqlx::Error),
    Thread,
    Busy,
    Stopped,
    StaleRevision,
    Limit,
    ExecutionLimit(Weight),
    InvalidState,
    Invariant(&'static str),
    Shell(provider_execution::Error),
    IncompleteCoverage,
    Reference(mock_provider::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sandbox worker: {}",
            match self {
                Self::Runtime(_) => "runtime/checkpoint failure",
                Self::Storage(_) => "storage failure",
                Self::Thread => "worker failed",
                Self::Busy => "queue full",
                Self::Stopped => "stopped",
                Self::StaleRevision => "stale revision",
                Self::Limit => "resource limit",
                Self::ExecutionLimit(_) => "execution resource limit",
                Self::InvalidState => "invalid stored state",
                Self::Invariant(code) => code,
                Self::Shell(error) => error.code(),
                Self::IncompleteCoverage => "incomplete contract coverage",
                Self::Reference(_) => "native reference failure",
            }
        )
    }
}
impl std::error::Error for Error {}
impl From<ProofError> for Error {
    fn from(e: ProofError) -> Self {
        Self::Runtime(e)
    }
}
impl From<provider_execution::Error> for Error {
    fn from(error: provider_execution::Error) -> Self {
        Self::Shell(error)
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Storage(e)
    }
}

pub enum Action {
    Instantiate { bundle: String, salt: Vec<u8> },
    Call { address: Account },
}
pub struct Invocation {
    pub caller: Account,
    pub action: Action,
    pub input: Vec<u8>,
    pub gas: Weight,
    pub timestamp_ms: u64,
}
impl Invocation {
    fn validate(&self) -> Result<(), Error> {
        self.validate_with_budget(GAS)
    }
    fn validate_with_budget(&self, limit: Weight) -> Result<(), Error> {
        if self.input.len() > MAX_INPUT || !self.gas.all_lte(limit) {
            return Err(Error::Limit);
        }
        if let Action::Instantiate { bundle, salt } = &self.action
            && (bundle.len() > 64 || salt.len() > 32)
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
}
/// Complete opaque transport state, including nonce, receipt and cursor. The
/// provider retains ownership of its codec and validation. Fixture bytes here
/// do not establish that the application shell has been integrated.
pub struct ShellChanges {
    pub success: Vec<u8>,
    pub rejection: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub enum Status {
    Success,
    Reverted,
    LanguageError,
    Dispatch(DispatchError),
}
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Outcome {
    pub status: Status,
    pub data: Vec<u8>,
    pub address: Option<Account>,
    pub gas_consumed: Weight,
}
#[derive(Debug)]
pub struct Diagnostics {
    pub revision: i64,
    pub root: H256,
    pub identity: Identity,
    pub checkpoint_bytes: usize,
}
pub struct Ticket<T>(oneshot::Receiver<Result<T, Error>>);
impl<T> Ticket<T> {
    pub async fn result(self) -> Result<T, Error> {
        self.0.await.map_err(|_| Error::Stopped)?
    }
}
type ReferenceLoader<'a> = &'a mut dyn FnMut() -> Result<mock_provider::ReferenceState, Error>;
type ReadReference = Box<dyn FnOnce(ReferenceLoader<'_>) + Send>;
enum Job {
    ReadReference(ReadReference),
    WholeDiagnostics(oneshot::Sender<Result<WholeDiagnostics, Error>>),
    Stop,
    Signed {
        call: SignedContractCallV1,
        now: UnixSeconds,
        inputs: Option<provider_execution::ExecutionInputs>,
        reply: oneshot::Sender<Result<OperationReceipt, Error>>,
    },
    Shell(oneshot::Sender<Result<Shell, Error>>),
    Archive(oneshot::Sender<Result<provider_execution::migration::ArchiveV1, Error>>),
    FinancialProjection {
        origin: AccountId32,
        reply: oneshot::Sender<Result<FinancialProjection, Error>>,
    },
    MoneyProjection {
        origin: AccountId32,
        reply: oneshot::Sender<Result<MoneyProjection, Error>>,
    },
    MarketplaceProjection {
        origin: AccountId32,
        reply: oneshot::Sender<Result<MarketplaceProjection, Error>>,
    },
    DirectoryProjection {
        origin: AccountId32,
        reply: oneshot::Sender<Result<DirectoryProjection, Error>>,
    },
    Execute {
        expected: i64,
        invocation: Invocation,
        shell: ShellChanges,
        reply: oneshot::Sender<Result<Outcome, Error>>,
    },
    Query {
        invocation: Invocation,
        reply: oneshot::Sender<Result<Outcome, Error>>,
    },
    Inspect(oneshot::Sender<Result<Diagnostics, Error>>),
}
#[derive(Clone)]
pub struct Client {
    mode: Mode,
    sender: SyncSender<Job>,
    stopping: Arc<AtomicBool>,
}
impl Client {
    /// Run an existing read model on the selected primary, serialized off Tokio cores.
    pub fn read_reference<T: Send + 'static>(
        &self,
        read: impl FnOnce(mock_provider::ReferenceState) -> mock_provider::Result<T> + Send + 'static,
    ) -> Result<Ticket<T>, Error> {
        whole::authority(self.mode)?;
        let (reply, receiver) = oneshot::channel();
        self.send(Job::ReadReference(Box::new(move |load| {
            respond_lazy(reply, || {
                load().and_then(|state| read(state).map_err(Error::Reference))
            })
        })))?;
        Ok(Ticket(receiver))
    }
    pub fn whole_diagnostics(&self) -> Result<Ticket<WholeDiagnostics>, Error> {
        whole::authority(self.mode)?;
        let (reply, receiver) = oneshot::channel();
        self.send(Job::WholeDiagnostics(reply))?;
        Ok(Ticket(receiver))
    }
    fn request_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        // A full queue already wakes the receiver. Disconnection means it has
        // stopped. Otherwise an explicit control message wakes an idle worker.
        match self.sender.try_send(Job::Stop) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }
    fn send(&self, job: Job) -> Result<(), Error> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        self.sender.try_send(job).map_err(|e| match e {
            TrySendError::Full(_) => Error::Busy,
            TrySendError::Disconnected(_) => Error::Stopped,
        })
    }
    pub fn execute(
        &self,
        expected: i64,
        invocation: Invocation,
        shell: ShellChanges,
    ) -> Result<Ticket<Outcome>, Error> {
        if !matches!(self.mode, Mode::Raw) {
            return Err(Error::InvalidState);
        }
        invocation.validate()?;
        if shell.success.len() > MAX_SHELL || shell.rejection.len() > MAX_SHELL {
            return Err(Error::Limit);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::Execute {
            expected,
            invocation,
            shell,
            reply,
        })?;
        Ok(Ticket(result))
    }
    pub fn query(&self, invocation: Invocation) -> Result<Ticket<Outcome>, Error> {
        if whole::authority(self.mode).is_ok() {
            return Err(Error::IncompleteCoverage);
        }
        invocation.validate()?;
        let (reply, result) = oneshot::channel();
        self.send(Job::Query { invocation, reply })?;
        Ok(Ticket(result))
    }
    /// Signed execution uses the real provider shell; no caller
    /// supplies a nonce update, receipt, outcome or event.
    pub fn execute_signed(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> Result<Ticket<OperationReceipt>, Error> {
        if matches!(self.mode, Mode::Raw) {
            return Err(Error::InvalidState);
        }
        if call.encoded_size() > MAX_INPUT {
            return Err(Error::Limit);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::Signed {
            call,
            now,
            inputs: None,
            reply,
        })?;
        Ok(Ticket(result))
    }
    /// Trusted comparison harness only: reuse the reference engine's captured
    /// inputs. HTTP execution captures its own inputs.
    pub fn execute_directory_with_inputs(
        &self,
        call: SignedContractCallV1,
        inputs: provider_execution::ExecutionInputs,
    ) -> Result<Ticket<OperationReceipt>, Error> {
        if !matches!(self.mode, Mode::Directory { .. }) {
            return Err(Error::InvalidState);
        }
        if call.encoded_size() > MAX_INPUT {
            return Err(Error::Limit);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::Signed {
            call,
            now: inputs.now,
            inputs: Some(inputs),
            reply,
        })?;
        Ok(Ticket(result))
    }
    /// Complete selected-primary archive for a drained administrative cutover.
    pub fn export_archive(
        &self,
    ) -> Result<Ticket<provider_execution::migration::ArchiveV1>, Error> {
        if !matches!(self.mode, Mode::Directory { whole: true, .. }) {
            return Err(Error::IncompleteCoverage);
        }
        let (reply, receiver) = oneshot::channel();
        self.send(Job::Archive(reply))?;
        Ok(Ticket(receiver))
    }
    pub fn directory_projection(
        &self,
        origin: AccountId32,
    ) -> Result<Ticket<DirectoryProjection>, Error> {
        if whole::authority(self.mode).is_ok() {
            return Err(Error::IncompleteCoverage);
        }
        if !matches!(self.mode, Mode::Directory { .. }) {
            return Err(Error::InvalidState);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::DirectoryProjection { origin, reply })?;
        Ok(Ticket(result))
    }
    /// Private fixture inspection, not a public provider or HTTP snapshot.
    pub fn shell(&self) -> Result<Ticket<Shell>, Error> {
        if matches!(self.mode, Mode::Raw) {
            return Err(Error::InvalidState);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::Shell(reply))?;
        Ok(Ticket(result))
    }
    /// Read-only M5 component projection. It does not claim money coverage.
    pub fn marketplace_projection(
        &self,
        origin: AccountId32,
    ) -> Result<Ticket<MarketplaceProjection>, Error> {
        if whole::authority(self.mode).is_ok() {
            return Err(Error::IncompleteCoverage);
        }
        if !matches!(self.mode, Mode::Directory { projects: true, .. }) {
            return Err(Error::IncompleteCoverage);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::MarketplaceProjection { origin, reply })?;
        Ok(Ticket(result))
    }
    /// Private M6 fixture read, never an HTTP endpoint.
    pub fn financial_projection(
        &self,
        origin: AccountId32,
    ) -> Result<Ticket<FinancialProjection>, Error> {
        if whole::authority(self.mode).is_ok() {
            return Err(Error::IncompleteCoverage);
        }
        if !matches!(self.mode, Mode::Directory { money: true, .. }) {
            return Err(Error::IncompleteCoverage);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::FinancialProjection { origin, reply })?;
        Ok(Ticket(result))
    }
    pub fn money_projection(&self, origin: AccountId32) -> Result<Ticket<MoneyProjection>, Error> {
        if whole::authority(self.mode).is_ok() {
            return Err(Error::IncompleteCoverage);
        }
        if !matches!(self.mode, Mode::Directory { money: true, .. }) {
            return Err(Error::IncompleteCoverage);
        }
        let (reply, result) = oneshot::channel();
        self.send(Job::MoneyProjection { origin, reply })?;
        Ok(Ticket(result))
    }
    pub fn diagnostics(&self) -> Result<Ticket<Diagnostics>, Error> {
        let (reply, result) = oneshot::channel();
        self.send(Job::Inspect(reply))?;
        Ok(Ticket(result))
    }
}

pub struct Worker {
    client: Client,
    join: Option<thread::JoinHandle<()>>,
    done: Option<oneshot::Receiver<Result<(), Error>>>,
}
impl Worker {
    /// Fresh whole-provider store. Authority is fixed for its lifetime.
    pub async fn import_whole(
        path: PathBuf,
        archive: provider_execution::migration::ArchiveV1,
        configuration: WholeConfiguration,
        authority: Authority,
    ) -> Result<Self, Error> {
        configuration.validate()?;
        // The retained native reference is compiled with the mock-seed policy.
        if !configuration.mock_funding {
            return Err(Error::InvalidState);
        }
        Self::start_with(
            path,
            InitialState::WholeImport {
                archive: Box::new(archive),
                configuration,
                authority,
            },
        )
        .await
    }
    /// Reopen only a compatible whole-provider store; never reseed missing state.
    pub async fn reopen_whole(
        path: PathBuf,
        configuration: WholeConfiguration,
        authority: Authority,
    ) -> Result<Self, Error> {
        configuration.validate()?;
        // The retained native reference is compiled with the mock-seed policy.
        if !configuration.mock_funding {
            return Err(Error::InvalidState);
        }
        Self::start_with(
            path,
            InitialState::WholeReopen {
                configuration,
                authority,
            },
        )
        .await
    }
    /// Creates candidate state only when no checkpoint exists. Corrupt or
    /// incompatible checkpoints fail startup; they are never replaced by genesis.
    pub async fn start(
        path: PathBuf,
        accounts: Vec<Account>,
        shell: Vec<u8>,
    ) -> Result<Self, Error> {
        if accounts.len() > 128 || shell.len() > MAX_SHELL {
            return Err(Error::Limit);
        }
        Self::start_with(path, InitialState::Raw { accounts, shell }).await
    }
    /// Start an explicit legacy-calendar fixture; this never selects sandbox
    /// authority for the app and reports other commands as incomplete coverage.
    pub async fn start_calendar_fixture(
        path: PathBuf,
        fixture: CalendarFixture,
    ) -> Result<Self, Error> {
        fixture.validate()?;
        Self::start_with(path, InitialState::Calendar(fixture)).await
    }
    /// Explicit fresh Directory/Calendar fixture. Other domains remain incomplete.
    pub async fn start_directory_fixture(
        path: PathBuf,
        fixture: DirectoryFixture,
    ) -> Result<Self, Error> {
        fixture.validate()?;
        Self::start_with(path, InitialState::Directory(fixture)).await
    }
    /// Import an already validated, drained private archive into a fresh store.
    /// The bridge compares every field; it does not replace native admission validation.
    pub async fn import_archive(
        path: PathBuf,
        archive: provider_execution::migration::ArchiveV1,
        configuration: WholeConfiguration,
    ) -> Result<Self, Error> {
        configuration.validate()?;
        Self::start_with(
            path,
            InitialState::Import {
                archive: Box::new(archive),
                configuration,
            },
        )
        .await
    }
    /// Existing sealed stores only. Missing or incompatible state never regenerates.
    pub async fn reopen(path: PathBuf, configuration: WholeConfiguration) -> Result<Self, Error> {
        configuration.validate()?;
        Self::start_with(path, InitialState::Reopen(configuration)).await
    }
    async fn start_with(path: PathBuf, initial: InitialState) -> Result<Self, Error> {
        let mode = initial.mode();
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let (ready, started) = oneshot::channel::<()>();
        let (done_tx, done) = oneshot::channel();
        let join = thread::Builder::new()
            .name("ink-v5-runtime".into())
            .spawn(move || {
                // Observe panics, fail closed, and drop all queued replies. A panic
                // never triggers automatic fallback or replay on another engine.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), Error> {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| Error::Thread)?;
                        let bundles = Bundles::pinned()?;
                        let pool = runtime.block_on(open(path, initial, &bundles))?;
                        if ready.send(()).is_err() {
                            runtime.block_on(pool.close());
                            return Ok(());
                        }
                        while !stop.load(Ordering::Acquire) {
                            let job = match receive_job(&receiver, &stop) {
                                Ok(Some(job)) => job,
                                Ok(None) | Err(_) => break,
                            };
                            match job {
                                Job::ReadReference(read) => {
                                    read(&mut || {
                                        runtime
                                            .block_on(whole::export(&pool, &bundles, mode))
                                            .and_then(|archive| whole::reference(mode, archive))
                                    });
                                }
                                Job::WholeDiagnostics(reply) => respond_lazy(reply, || {
                                    runtime.block_on(whole::diagnostics(&pool, mode))
                                }),
                                Job::Stop => break,
                                Job::Signed {
                                    call,
                                    now,
                                    inputs,
                                    reply,
                                } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(async {
                                                match mode {
                                                    Mode::Directory {
                                                        authority: Some(_), ..
                                                    } => {
                                                        whole::execute(
                                                            &pool, &bundles, mode, call, now,
                                                            inputs,
                                                        )
                                                        .await
                                                    }
                                                    Mode::Directory { .. } => {
                                                        directory::execute(
                                                            &pool, &bundles, mode, call, now,
                                                            inputs,
                                                        )
                                                        .await
                                                    }
                                                    _ => {
                                                        signed::execute(
                                                            &pool, &bundles, mode, call, now,
                                                        )
                                                        .await
                                                    }
                                                }
                                            }),
                                        );
                                    }
                                }
                                Job::Archive(reply) => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(async {
                                                if whole::authority(mode).is_ok() {
                                                    whole::export(&pool, &bundles, mode).await
                                                } else {
                                                    migration::export(&pool, &bundles, mode).await
                                                }
                                            }),
                                        );
                                    }
                                }
                                Job::Shell(reply) => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(async {
                                                match mode {
                                                    Mode::Directory {
                                                        authority: Some(_), ..
                                                    } => Ok(whole::export(&pool, &bundles, mode)
                                                        .await?
                                                        .checkpoint
                                                        .shell()),
                                                    Mode::Directory { .. } => {
                                                        directory::read_shell(&pool, &bundles, mode)
                                                            .await
                                                    }
                                                    _ => {
                                                        signed::read_shell(&pool, &bundles, mode)
                                                            .await
                                                    }
                                                }
                                            }),
                                        );
                                    }
                                }
                                Job::FinancialProjection { origin, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(directory::financial_projection(
                                                &pool, &bundles, mode, origin,
                                            )),
                                        );
                                    }
                                }
                                Job::MoneyProjection { origin, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(directory::money_projection(
                                                &pool, &bundles, mode, origin,
                                            )),
                                        );
                                    }
                                }
                                Job::MarketplaceProjection { origin, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(directory::marketplace_projection(
                                                &pool, &bundles, mode, origin,
                                            )),
                                        );
                                    }
                                }
                                Job::DirectoryProjection { origin, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(directory::projection(
                                                &pool, &bundles, mode, origin,
                                            )),
                                        );
                                    }
                                }
                                Job::Execute {
                                    expected,
                                    invocation,
                                    shell,
                                    reply,
                                } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(execute(
                                                &pool, &bundles, expected, invocation, shell,
                                            )),
                                        );
                                    }
                                }
                                Job::Query { invocation, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime
                                                .block_on(query(&pool, &bundles, mode, invocation)),
                                        );
                                    }
                                }
                                Job::Inspect(reply) => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(inspect(
                                                &pool,
                                                bundles.identity(),
                                                mode,
                                            )),
                                        );
                                    }
                                }
                            }
                        }
                        runtime.block_on(pool.close());
                        Ok(())
                    },
                ))
                .unwrap_or(Err(Error::Thread));
                if result.is_err() {
                    eprintln!("ink-v5 runtime worker stopped with an infrastructure failure");
                }
                // A dropped receiver is an observed caller cancellation, not a worker error.
                if done_tx.send(result).is_err() {
                    eprintln!("ink-v5 runtime worker owner dropped before shutdown");
                }
            })
            .map_err(|_| Error::Thread)?;
        let mut worker = Self {
            client: Client {
                mode,
                sender,
                stopping,
            },
            join: Some(join),
            done: Some(done),
        };
        match started.await {
            Ok(()) => Ok(worker),
            _ => {
                let result = worker.finish().await;
                Err(result.err().unwrap_or(Error::Stopped))
            }
        }
    }
    pub fn client(&self) -> Client {
        self.client.clone()
    }
    pub async fn shutdown(mut self) -> Result<(), Error> {
        self.finish().await
    }
    async fn finish(&mut self) -> Result<(), Error> {
        self.client.request_stop();
        let outcome = match self.done.take() {
            Some(done) => done.await.map_err(|_| Error::Thread)?,
            None => Ok(()),
        };
        if let Some(join) = self.join.take() {
            tokio::task::spawn_blocking(move || join.join().map_err(|_| Error::Thread))
                .await
                .map_err(|_| Error::Thread)??;
        }
        outcome
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Explicit async shutdown observes the join. Dropping still stops admission
        // and signals the dedicated thread, which observes/logs its final result.
        self.client.request_stop();
    }
}
fn receive_job(
    receiver: &Receiver<Job>,
    stopping: &AtomicBool,
) -> Result<Option<Job>, mpsc::RecvError> {
    let job = receiver.recv()?;
    // This is the request-start boundary. Shutdown may have begun while recv
    // waited, after the loop's earlier check. Queued work must remain unstarted.
    Ok((!stopping.load(Ordering::Acquire)).then_some(job))
}
fn respond_lazy<T>(
    reply: oneshot::Sender<Result<T, Error>>,
    work: impl FnOnce() -> Result<T, Error>,
) {
    if !reply.is_closed() {
        respond(reply, work());
    }
}
fn respond<T>(reply: oneshot::Sender<Result<T, Error>>, result: Result<T, Error>) {
    if reply.send(result).is_err() {
        // Work already started must finish its atomic commit even if HTTP went away.
        // The caller must read durable state/replay via the shell, never assume rollback.
        eprintln!("ink-v5 runtime reply receiver closed after execution");
    }
}

async fn open(
    path: PathBuf,
    initial: InitialState,
    bundles: &Bundles,
) -> Result<SqlitePool, Error> {
    let mode = initial.mode();
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(!matches!(
            &initial,
            InitialState::Reopen(_) | InitialState::WholeReopen { .. }
        ))
        .busy_timeout(Duration::from_secs(5))
        .synchronous(SqliteSynchronous::Full);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(6))
        .connect_with(options)
        .await?;
    let foreign_tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT IN ('candidate_state','candidate_reference') AND name NOT GLOB 'sqlite_*'")
        .fetch_one(&pool).await?;
    if foreign_tables != 0 {
        return Err(Error::InvalidState);
    }
    sqlx::query("CREATE TABLE IF NOT EXISTS candidate_state (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), revision INTEGER NOT NULL CHECK(revision >= 0), mode TEXT NOT NULL CHECK(mode IN ('raw-fixture-v1','signed-calendar-v1','signed-directory-v1')), runtime BLOB NOT NULL CHECK(length(runtime) <= 67108864), shell BLOB NOT NULL CHECK(length(shell) <= 67108864), outcome BLOB NOT NULL CHECK(length(outcome) <= 524288))").execute(&pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM candidate_state")
        .fetch_one(&mut *tx)
        .await?;
    if (matches!(
        &initial,
        InitialState::Import { .. } | InitialState::WholeImport { .. }
    ) && exists != 0)
        || (matches!(
            &initial,
            InitialState::Reopen(_) | InitialState::WholeReopen { .. }
        ) && exists == 0)
    {
        return Err(Error::InvalidState);
    }
    let mut reference_archive = None;
    if exists == 0 {
        let (ext, shell) = match initial {
            InitialState::Raw { accounts, shell } => (new_externalities(accounts)?, shell),
            InitialState::Calendar(fixture) => signed::initialize(bundles, fixture)?,
            InitialState::Directory(fixture) => directory::initialize(bundles, fixture)?,
            InitialState::Import {
                archive,
                configuration,
            } => migration::initialize(bundles, *archive, configuration)?,
            InitialState::WholeImport {
                archive,
                configuration,
                ..
            } => {
                whole::reference(mode, (*archive).clone())?;
                reference_archive = Some((*archive).clone());
                let (ext, bytes) = migration::initialize(bundles, *archive, configuration)?;
                let mut state: directory::State =
                    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidState)?;
                if let Mode::Directory { configuration, .. } = mode {
                    state.configuration = configuration;
                }
                (ext, directory::encode(&state)?)
            }
            InitialState::Reopen(_) | InitialState::WholeReopen { .. } => {
                return Err(Error::InvalidState);
            }
        };
        let checkpoint = Checkpoint::capture(ext)?.to_bytes()?;
        sqlx::query("INSERT INTO candidate_state (singleton, revision, mode, runtime, shell, outcome) VALUES (1, 0, ?, ?, ?, ?)")
            .bind(mode.key())
            .bind(checkpoint)
            .bind(shell)
            .bind(Vec::<u8>::new())
            .execute(&mut *tx)
            .await?;
    }
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    if let Mode::Calendar { root, .. } = mode {
        signed::validate(&stored, bundles, root)?;
    }
    if matches!(mode, Mode::Directory { .. }) {
        directory::validate(&stored, bundles, mode)?;
    }
    if whole::authority(mode).is_ok() {
        whole::initialize(&mut tx, &stored, bundles, mode, reference_archive).await?;
    }
    tx.commit().await?;
    Ok(pool)
}
struct Stored {
    revision: i64,
    checkpoint: Checkpoint,
    bytes: usize,
    shell: Vec<u8>,
}
async fn load(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    identity: Identity,
    mode: Mode,
) -> Result<Stored, Error> {
    // Check lengths in SQL before materializing potentially corrupt blobs.
    let row = sqlx::query("SELECT revision, runtime, shell FROM candidate_state WHERE singleton = 1 AND mode = ? AND revision >= 0 AND length(runtime) <= 67108864 AND length(shell) <= 67108864 AND length(outcome) <= 524288").bind(mode.key()).fetch_optional(&mut **tx).await?.ok_or(Error::InvalidState)?;
    let revision: i64 = row.try_get("revision")?;
    let bytes: Vec<u8> = row.try_get("runtime")?;
    Ok(Stored {
        revision,
        checkpoint: Checkpoint::decode_for(&bytes, identity)?,
        bytes: bytes.len(),
        shell: row.try_get("shell")?,
    })
}
fn run(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    invocation: Invocation,
) -> Result<Outcome, Error> {
    let code = if let Action::Instantiate { bundle, .. } = &invocation.action {
        Some(bundles.code(bundle)?.to_vec())
    } else {
        None
    };
    ext.execute_with(|| {
        Timestamp::set_timestamp(invocation.timestamp_ms);
        let (result, gas_consumed) = match invocation.action {
            Action::Call { address } => {
                let result = Contracts::bare_call(
                    invocation.caller,
                    address,
                    0,
                    invocation.gas,
                    None,
                    invocation.input,
                    DebugInfo::Skip,
                    CollectEvents::Skip,
                    Determinism::Enforced,
                );
                (
                    result.result.map(|value| (None, value)),
                    result.gas_consumed,
                )
            }
            Action::Instantiate { salt, .. } => {
                let result = Contracts::bare_instantiate(
                    invocation.caller,
                    0,
                    invocation.gas,
                    None,
                    Code::Upload(code.ok_or(Error::InvalidState)?),
                    invocation.input,
                    salt,
                    DebugInfo::Skip,
                    CollectEvents::Skip,
                );
                (
                    result
                        .result
                        .map(|value| (Some(value.account_id), value.result)),
                    result.gas_consumed,
                )
            }
        };
        match result {
            Err(error) => Ok(Outcome {
                status: Status::Dispatch(error),
                data: Vec::new(),
                address: None,
                gas_consumed,
            }),
            Ok((address, value)) => {
                if value.data.len() > MAX_INPUT {
                    return Err(Error::Limit);
                }
                let status = match value.data.first() {
                    Some(0) if !value.did_revert() => Status::Success,
                    Some(0) => Status::Reverted,
                    _ => Status::LanguageError,
                };
                Ok(Outcome {
                    status,
                    data: value.data,
                    address,
                    gas_consumed,
                })
            }
        }
    })
}
async fn execute(
    pool: &SqlitePool,
    bundles: &Bundles,
    expected: i64,
    invocation: Invocation,
    shell: ShellChanges,
) -> Result<Outcome, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), Mode::Raw).await?;
    if stored.revision != expected {
        tx.rollback().await?;
        return Err(Error::StaleRevision);
    }
    let revision = expected.checked_add(1).ok_or(Error::Limit)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    let outcome = run(&mut ext, bundles, invocation)?;
    let (checkpoint, shell) = if outcome.status == Status::Success {
        (Checkpoint::capture(ext)?, shell.success)
    } else {
        (stored.checkpoint, shell.rejection)
    };
    let changed = sqlx::query("UPDATE candidate_state SET revision = ?, runtime = ?, shell = ?, outcome = ? WHERE singleton = 1 AND revision = ?").bind(revision).bind(checkpoint.to_bytes()?).bind(shell).bind(outcome.encode()).bind(expected).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    tx.commit().await?;
    Ok(outcome)
}
async fn query(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    invocation: Invocation,
) -> Result<Outcome, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let outcome = run(&mut stored.checkpoint.restore()?, bundles, invocation)?;
    tx.rollback().await?;
    Ok(outcome)
}
async fn inspect(pool: &SqlitePool, identity: Identity, mode: Mode) -> Result<Diagnostics, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, identity, mode).await?;
    tx.rollback().await?;
    Ok(Diagnostics {
        revision: stored.revision,
        root: stored.checkpoint.root(),
        identity,
        checkpoint_bytes: stored.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_while_receiving_drops_queued_work() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let stop = AtomicBool::new(false);
        let reached = std::sync::Barrier::new(2);
        let (reply, mut result) = oneshot::channel();
        thread::scope(|scope| {
            let stopper = &stop;
            let gate = &reached;
            let join = scope.spawn(move || {
                // Reproduce the old loop's pre-receive check before shutdown.
                assert!(!stopper.load(Ordering::Acquire));
                gate.wait();
                receive_job(&receiver, stopper)
            });
            reached.wait();
            stop.store(true, Ordering::Release);
            assert!(sender.send(Job::Inspect(reply)).is_ok());
            assert!(matches!(join.join().unwrap(), Ok(None)));
        });
        assert!(matches!(
            result.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
    #[test]
    fn canceled_queued_reads_skip_archive_loading_and_diagnostics() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let client = Client {
            mode: whole::mode(
                &WholeConfiguration {
                    root: AccountId32::from_bytes([1; 32]),
                    generation: generated_contracts::ProviderInstanceId::from_bytes([2; 16]),
                    mock_funding: true,
                    gas_limit: crate::WHOLE_GAS,
                },
                Authority::Sandbox,
            ),
            sender,
            stopping: Arc::new(AtomicBool::new(false)),
        };
        let reached = std::sync::Barrier::new(2);
        let release = std::sync::Barrier::new(2);
        thread::scope(|scope| {
            let gate = &reached;
            let proceed = &release;
            let join = scope.spawn(move || {
                gate.wait();
                proceed.wait();
                match receiver.recv().unwrap() {
                    Job::ReadReference(read) => {
                        read(&mut || panic!("canceled read must not export an archive"))
                    }
                    _ => panic!("expected queued read"),
                }
                match receiver.recv().unwrap() {
                    Job::WholeDiagnostics(reply) => respond_lazy(reply, || {
                        panic!("canceled diagnostics must not read storage")
                    }),
                    _ => panic!("expected queued diagnostics"),
                }
            });
            reached.wait();
            let read = client
                .read_reference::<()>(|_| panic!("canceled read model must not run"))
                .unwrap();
            let diagnostics = client.whole_diagnostics().unwrap();
            drop(read);
            drop(diagnostics);
            release.wait();
            join.join().unwrap();
        });
    }
    #[test]
    fn full_queue_and_shutdown_reject_admission() {
        let (sender, _receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let client = Client {
            mode: Mode::Raw,
            sender,
            stopping: Arc::new(AtomicBool::new(false)),
        };
        let mut tickets = Vec::new();
        for _ in 0..QUEUE_CAPACITY {
            tickets.push(client.diagnostics().unwrap());
        }
        assert!(matches!(client.diagnostics(), Err(Error::Busy)));
        client.stopping.store(true, Ordering::Release);
        assert!(matches!(client.diagnostics(), Err(Error::Stopped)));
    }
}
