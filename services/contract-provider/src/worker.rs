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
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism};
use parity_scale_codec::{Decode, Encode};
use protocol::{AccountId32, OperationReceipt, SignedContractCallV1, UnixSeconds};
use provider::execution::Shell;
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
mod configuration;
mod dao;
mod facts;
mod reads;
pub use configuration::Configuration;
mod project_storage;
mod projects;

#[derive(Clone, Copy)]
enum Mode {
    Raw,
    Contracts {
        root: AccountId32,
        gas: Weight,
        configuration: H256,
    },
}
impl Mode {
    fn key(self) -> &'static str {
        match self {
            Self::Raw => "fixture",
            Self::Contracts { .. } => "contracts",
        }
    }
}
enum InitialState {
    Raw {
        accounts: Vec<Account>,
        shell: Vec<u8>,
    },
    Provider(Configuration),
}
impl InitialState {
    fn mode(&self) -> Mode {
        match self {
            Self::Raw { .. } => Mode::Raw,
            Self::Provider(configuration) => configuration.mode(),
        }
    }
}

pub const QUEUE_CAPACITY: usize = 8;
// Retain the existing unsigned limit plus signature/ABI framing overhead.
pub const MAX_INPUT: usize = protocol::MAX_SIGNABLE_BYTES + 1024;
pub const MAX_SHELL: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Runtime(ProofError),
    Storage(sqlx::Error),
    Io(std::io::Error),
    Thread,
    Busy,
    Stopped,
    StaleRevision,
    Limit,
    ExecutionLimit(Weight),
    InvalidState,
    Invariant(&'static str),
    Shell(provider::execution::Error),
    IncompleteCoverage,
    Provider(provider::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sandbox worker: {}",
            match self {
                Self::Runtime(_) => "runtime/checkpoint failure",
                Self::Storage(_) => "storage failure",
                Self::Io(_) => "filesystem failure",
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
                Self::Provider(_) => "provider read failure",
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
impl From<provider::execution::Error> for Error {
    fn from(error: provider::execution::Error) -> Self {
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
type Read = Box<dyn FnOnce(&mut dyn FnMut() -> Result<Stored, Error>, Mode) + Send>;
type ShellRead = Box<dyn FnOnce(&mut dyn FnMut() -> Result<Shell, Error>) + Send>;
enum Job {
    Read(Read),
    Stop,
    Signed {
        call: SignedContractCallV1,
        now: UnixSeconds,
        reply: oneshot::Sender<Result<OperationReceipt, Error>>,
    },
    Shell(ShellRead),
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
    /// Read one committed revision on the dedicated worker. Shell reads do not load domain state.
    pub(crate) fn read<T: Send + 'static>(
        &self,
        read: impl FnOnce(&mut reads::Reader) -> provider::Result<T> + Send + 'static,
    ) -> Result<Ticket<T>, Error> {
        if !matches!(self.mode, Mode::Contracts { .. }) {
            return Err(Error::InvalidState);
        }
        let (reply, receiver) = oneshot::channel();
        self.send(Job::Read(Box::new(move |stored, mode| {
            respond_lazy(reply, || {
                let mut reader = reads::Reader::new(stored()?, mode)?;
                read(&mut reader).map_err(Error::Provider)
            });
        })))?;
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
        self.send(Job::Signed { call, now, reply })?;
        Ok(Ticket(result))
    }
    /// Private fixture inspection, not a public provider or HTTP snapshot.
    pub fn shell(&self) -> Result<Ticket<Shell>, Error> {
        self.read_shell(Ok)
    }
    pub(crate) fn read_shell<T: Send + 'static>(
        &self,
        read: impl FnOnce(Shell) -> Result<T, Error> + Send + 'static,
    ) -> Result<Ticket<T>, Error> {
        if matches!(self.mode, Mode::Raw) {
            return Err(Error::InvalidState);
        }
        let (reply, receiver) = oneshot::channel();
        self.send(Job::Shell(Box::new(move |load| {
            respond_lazy(reply, || read(load()?));
        })))?;
        Ok(Ticket(receiver))
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
    /// Open current state or initialize an empty installation through contract constructors.
    pub async fn open(path: PathBuf, configuration: Configuration) -> Result<Self, Error> {
        configuration.validate()?;
        Self::start_with(path, InitialState::Provider(configuration)).await
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
                                Job::Read(read) => {
                                    let mut load_stored = || {
                                        runtime.block_on(async {
                                            let mut tx = pool.begin().await?;
                                            let stored =
                                                load(&mut tx, bundles.identity(), mode).await?;
                                            tx.rollback().await?;
                                            Ok(stored)
                                        })
                                    };
                                    read(&mut load_stored, mode);
                                }
                                Job::Stop => break,
                                Job::Signed { call, now, reply } => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(async {
                                                match mode {
                                                    Mode::Contracts { .. } => {
                                                        dao::execute(
                                                            &pool, &bundles, mode, call, now,
                                                        )
                                                        .await
                                                    }
                                                    Mode::Raw => Err(Error::InvalidState),
                                                }
                                            }),
                                        );
                                    }
                                }
                                Job::Shell(read) => {
                                    read(&mut || runtime.block_on(dao::read_shell(&pool, mode)));
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
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file.mode(0o600);
    }
    let fresh = match file.open(&path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(Error::Io(error)),
    };

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .synchronous(SqliteSynchronous::Full);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(6))
        .connect_with(options)
        .await?;
    let foreign_tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT IN ('provider_state') AND name NOT GLOB 'sqlite_*'")
        .fetch_one(&pool).await?;
    if foreign_tables != 0 {
        return Err(Error::InvalidState);
    }
    let had_state: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='provider_state')",
    )
    .fetch_one(&pool)
    .await?;
    if !fresh && !had_state {
        return Err(Error::InvalidState);
    }
    sqlx::query("CREATE TABLE IF NOT EXISTS provider_state (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), revision INTEGER NOT NULL CHECK(revision >= 0), mode TEXT NOT NULL CHECK(mode IN ('fixture','contracts')), runtime BLOB NOT NULL CHECK(length(runtime) <= 67108864), shell BLOB NOT NULL CHECK(length(shell) <= 67108864), outcome BLOB NOT NULL CHECK(length(outcome) <= 524288))").execute(&pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM provider_state")
        .fetch_one(&mut *tx)
        .await?;
    if had_state && exists != 1 {
        return Err(Error::InvalidState);
    }
    if exists == 0 {
        let (ext, shell) = match initial {
            InitialState::Raw { accounts, shell } => (new_externalities(accounts)?, shell),
            InitialState::Provider(configuration) => configuration.initialize(bundles)?,
        };
        let checkpoint = Checkpoint::capture(ext)?.to_bytes()?;
        sqlx::query("INSERT INTO provider_state (singleton, revision, mode, runtime, shell, outcome) VALUES (1, 0, ?, ?, ?, ?)")
            .bind(mode.key())
            .bind(checkpoint)
            .bind(shell)
            .bind(Vec::<u8>::new())
            .execute(&mut *tx)
            .await?;
    }
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    if matches!(mode, Mode::Contracts { .. }) {
        dao::validate(&stored, bundles, mode)?;
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
    let row = sqlx::query("SELECT revision, runtime, shell FROM provider_state WHERE singleton = 1 AND mode = ? AND revision >= 0 AND length(runtime) <= 67108864 AND length(shell) <= 67108864 AND length(outcome) <= 524288").bind(mode.key()).fetch_optional(&mut **tx).await?.ok_or(Error::InvalidState)?;
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
    let changed = sqlx::query("UPDATE provider_state SET revision = ?, runtime = ?, shell = ?, outcome = ? WHERE singleton = 1 AND revision = ?").bind(revision).bind(checkpoint.to_bytes()?).bind(shell).bind(outcome.encode()).bind(expected).execute(&mut *tx).await?;
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
    #[tokio::test]
    async fn shell_projection_runs_on_the_owned_worker_thread() {
        let path =
            std::env::temp_dir().join(format!("shell-read-worker-{}.sqlite", std::process::id()));
        assert!(!path.exists());
        let worker = Worker::open(
            path.clone(),
            Configuration {
                root: AccountId32::from_bytes([1; 32]),
                mock_funding: false,
                gas_limit: crate::EXECUTION_GAS,
            },
        )
        .await
        .unwrap();
        let name = worker
            .client()
            .read_shell(|_| Ok(std::thread::current().name().unwrap().to_owned()))
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(name, "ink-v5-runtime");
        worker.shutdown().await.unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn canceled_queued_reads_skip_decoding_and_model_work() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let mode = Configuration {
            root: AccountId32::from_bytes([1; 32]),
            mock_funding: true,
            gas_limit: crate::EXECUTION_GAS,
        }
        .mode();
        let client = Client {
            mode,
            sender,
            stopping: Arc::new(AtomicBool::new(false)),
        };
        let ticket = client
            .read::<()>(|_| panic!("canceled read must not run"))
            .unwrap();
        drop(ticket);
        match receiver.recv().unwrap() {
            Job::Read(read) => read(
                &mut || panic!("canceled read must not load or decode"),
                mode,
            ),
            _ => panic!("expected read"),
        }
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
