//! Private candidate execution. This module is not linked into the provider.
//!
//! The future shell bridge supplies its complete success/rejection envelopes.
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

pub const QUEUE_CAPACITY: usize = 8;
pub const MAX_INPUT: usize = 64 * 1024;
pub const MAX_SHELL: usize = 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Runtime(ProofError),
    Storage(sqlx::Error),
    Thread,
    Busy,
    Stopped,
    StaleRevision,
    Limit,
    InvalidState,
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
                Self::InvalidState => "invalid stored state",
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
        if self.input.len() > MAX_INPUT || !self.gas.all_lte(GAS) {
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
enum Job {
    Stop,
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
    sender: SyncSender<Job>,
    stopping: Arc<AtomicBool>,
}
impl Client {
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
                        let pool =
                            runtime.block_on(open(path, accounts, shell, bundles.identity()))?;
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
                                Job::Stop => break,
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
                                            runtime.block_on(query(&pool, &bundles, invocation)),
                                        );
                                    }
                                }
                                Job::Inspect(reply) => {
                                    if !reply.is_closed() {
                                        respond(
                                            reply,
                                            runtime.block_on(inspect(&pool, bundles.identity())),
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
            client: Client { sender, stopping },
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
fn respond<T>(reply: oneshot::Sender<Result<T, Error>>, result: Result<T, Error>) {
    if reply.send(result).is_err() {
        // Work already started must finish its atomic commit even if HTTP went away.
        // The caller must read durable state/replay via the shell, never assume rollback.
        eprintln!("ink-v5 runtime reply receiver closed after execution");
    }
}

async fn open(
    path: PathBuf,
    accounts: Vec<Account>,
    shell: Vec<u8>,
    identity: Identity,
) -> Result<SqlitePool, Error> {
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
    sqlx::query("CREATE TABLE IF NOT EXISTS candidate_state (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), revision INTEGER NOT NULL CHECK(revision >= 0), runtime BLOB NOT NULL CHECK(length(runtime) <= 67108864), shell BLOB NOT NULL CHECK(length(shell) <= 1048576), outcome BLOB NOT NULL CHECK(length(outcome) <= 131072))").execute(&pool).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM candidate_state")
        .fetch_one(&mut *tx)
        .await?;
    if exists == 0 {
        let checkpoint = Checkpoint::capture(new_externalities(accounts)?)?.to_bytes()?;
        sqlx::query("INSERT INTO candidate_state VALUES (1, 0, ?, ?, ?)")
            .bind(checkpoint)
            .bind(shell)
            .bind(Vec::<u8>::new())
            .execute(&mut *tx)
            .await?;
    }
    load(&mut tx, identity).await?;
    tx.commit().await?;
    Ok(pool)
}
struct Stored {
    revision: i64,
    checkpoint: Checkpoint,
    bytes: usize,
}
async fn load(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    identity: Identity,
) -> Result<Stored, Error> {
    // Check lengths in SQL before materializing potentially corrupt blobs.
    let row = sqlx::query("SELECT revision, runtime FROM candidate_state WHERE singleton = 1 AND revision >= 0 AND length(runtime) <= 67108864 AND length(shell) <= 1048576 AND length(outcome) <= 131072").fetch_optional(&mut **tx).await?.ok_or(Error::InvalidState)?;
    let revision: i64 = row.try_get("revision")?;
    let bytes: Vec<u8> = row.try_get("runtime")?;
    Ok(Stored {
        revision,
        checkpoint: Checkpoint::decode_for(&bytes, identity)?,
        bytes: bytes.len(),
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
    let stored = load(&mut tx, bundles.identity()).await?;
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
    invocation: Invocation,
) -> Result<Outcome, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity()).await?;
    let outcome = run(&mut stored.checkpoint.restore()?, bundles, invocation)?;
    tx.rollback().await?;
    Ok(outcome)
}
async fn inspect(pool: &SqlitePool, identity: Identity) -> Result<Diagnostics, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, identity).await?;
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
    fn full_queue_and_shutdown_reject_admission() {
        let (sender, _receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let client = Client {
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
