use generated_contracts::provider_core::CoreDisputeView;
use generated_contracts::provider_core::{CoreSnapshotV4, ReasonPageV1, SkillRequestCore};
use std::sync::Arc;

use generated_contracts::{
    AccountId32, AccountNonce, DepositView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, SignedContractCallV1, UnixSeconds, WithdrawalView,
};

use crate::{
    Error, Result,
    domain::{State, VerifiedCall},
};

enum Backend {
    #[cfg(feature = "storage-memory")]
    Memory(Box<tokio::sync::Mutex<State>>),
    #[cfg(feature = "storage-sqlite")]
    Sqlite(sqlx::SqlitePool),
}

/// Authoritative contract runtime. Clones share storage; no caller can mutate a
/// snapshot or bypass signed-envelope validation.
#[derive(Clone)]
pub struct Provider {
    backend: Arc<Backend>,
    #[cfg(feature = "storage-sqlite")]
    root: AccountId32,
}

impl Provider {
    /// Requests are visible only to their worker or the configured system account.
    /// Bounded owner-only reason facts; content stays in the adapter.
    pub async fn lifecycle_reasons(&self, after: u64, limit: usize) -> Result<ReasonPageV1> {
        self.read().await?.lifecycle_reasons(after, limit)
    }
    /// Requests visible to their worker or the configured system account.
    pub async fn skill_requests(&self, origin: AccountId32) -> Result<Vec<SkillRequestCore>> {
        Ok(self.read().await?.skill_requests(origin))
    }

    /// Read one deposit with the same owner/system policy as the mock contract.
    pub async fn bramp_deposit(&self, origin: AccountId32, id: EntityId) -> Result<DepositView> {
        self.read().await?.bramp_deposit(origin, id)
    }

    /// Read one withdrawal without exposing another account's request.
    pub async fn bramp_withdrawal(
        &self,
        origin: AccountId32,
        id: EntityId,
    ) -> Result<WithdrawalView> {
        self.read().await?.bramp_withdrawal(origin, id)
    }

    /// Explicit public case projection, without internal state or receipts.
    pub async fn dispute(&self, id: generated_contracts::EntityId) -> Result<CoreDisputeView> {
        self.read().await?.dispute_view(id)
    }
    /// New disposable memory generation; recreating it invalidates old signatures.
    #[cfg(feature = "storage-memory")]
    pub fn memory(root: AccountId32) -> Result<Self> {
        Ok(Self {
            backend: Arc::new(Backend::Memory(Box::new(tokio::sync::Mutex::new(
                State::new(root)?,
            )))),
            #[cfg(feature = "storage-sqlite")]
            root,
        })
    }

    /// Retain an `SQLite` generation until its database is explicitly discarded.
    /// Startup never silently changes its configured root or overwrites seed edits.
    #[cfg(feature = "storage-sqlite")]
    pub async fn sqlite(url: &str, root: AccountId32) -> Result<Self> {
        Self::open_sqlite(url, root, None).await
    }

    /// Import into a fresh `SQLite` provider only. Existing state is never replaced.
    /// The caller must pause/drain the source before exporting the cutover archive.
    #[cfg(feature = "storage-sqlite")]
    pub async fn sqlite_from_archive(
        url: &str,
        root: AccountId32,
        archive: provider_execution::migration::CoreArchiveV4,
    ) -> Result<Self> {
        Self::open_sqlite(url, root, Some(archive)).await
    }

    #[cfg(feature = "storage-sqlite")]
    async fn open_sqlite(
        url: &str,
        root: AccountId32,
        archive: Option<provider_execution::migration::CoreArchiveV4>,
    ) -> Result<Self> {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::{str::FromStr, time::Duration};
        let options = SqliteConnectOptions::from_str(url)
            .map_err(|_| Error::bad("invalid_database_url"))?
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await
            .map_err(|_| Error::internal())?;
        sqlx::query("CREATE TABLE IF NOT EXISTS provider_state (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), format_version INTEGER NOT NULL CHECK (format_version = 1), state BLOB NOT NULL)")
            .execute(&pool).await.map_err(|_| Error::internal())?;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|_| Error::internal())?;
        let row: Option<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT format_version, state FROM provider_state WHERE singleton = 1")
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| Error::internal())?;
        let importing = archive.is_some();
        if importing && row.is_some() {
            return Err(Error::domain("migration_destination_not_empty"));
        }
        let row = match row {
            Some((1, bytes)) => Some(bytes),
            Some(_) => return Err(Error::domain("state_configuration_mismatch")),
            None => None,
        };
        sqlx::query("CREATE TABLE IF NOT EXISTS provider_migration (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), imported INTEGER NOT NULL CHECK(imported = 1))")
            .execute(&mut *tx).await.map_err(|_| Error::internal())?;
        let previously_imported: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM provider_migration WHERE singleton = 1)",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Error::internal())?;
        if previously_imported && row.is_none() {
            return Err(Error::domain("state_configuration_mismatch"));
        }
        let bytes = tokio::task::spawn_blocking(move || {
            let imported = archive.is_some() || previously_imported;
            let mut state = match (archive, row) {
                (Some(_), Some(_)) => return Err(Error::domain("migration_destination_not_empty")),
                (Some(archive), None) => State::from_archive(archive, root)?,
                (None, Some(bytes)) => State::restore(&bytes, root)?,
                (None, None) => State::new(root)?,
            };
            if !imported {
                state.seed();
            }
            state.encode()
        })
        .await
        .map_err(|_| Error::internal())??;
        sqlx::query("INSERT INTO provider_state (singleton, format_version, state) VALUES (1, 1, ?) ON CONFLICT(singleton) DO UPDATE SET state = excluded.state")
            .bind(bytes).execute(&mut *tx).await.map_err(|_| Error::internal())?;
        if importing {
            sqlx::query("INSERT INTO provider_migration (singleton, imported) VALUES (1, 1)")
                .execute(&mut *tx)
                .await
                .map_err(|_| Error::internal())?;
        }
        tx.commit().await.map_err(|_| Error::internal())?;
        Ok(Self {
            backend: Arc::new(Backend::Sqlite(pool)),
            root,
        })
    }

    /// Complete private archive for a paused migration; no HTTP route exposes it.
    /// The caller must drain writes before taking the cutover archive.
    pub async fn archive(&self) -> Result<provider_execution::migration::CoreArchiveV4> {
        let state = self.read().await?;
        tokio::task::spawn_blocking(move || state.archive())
            .await
            .map_err(|_| Error::internal())?
    }

    /// Restore current exported state into a fresh memory provider generation.
    /// Generation, replay history and nonces are retained, never regenerated.
    #[cfg(feature = "storage-memory")]
    pub async fn memory_from_archive(
        root: AccountId32,
        archive: provider_execution::migration::CoreArchiveV4,
    ) -> Result<Self> {
        let state = tokio::task::spawn_blocking(move || State::from_archive(archive, root))
            .await
            .map_err(|_| Error::internal())??;
        Ok(Self {
            backend: Arc::new(Backend::Memory(Box::new(tokio::sync::Mutex::new(state)))),
            #[cfg(feature = "storage-sqlite")]
            root,
        })
    }

    async fn read(&self) -> Result<State> {
        match self.backend.as_ref() {
            #[cfg(feature = "storage-memory")]
            Backend::Memory(state) => Ok(state.lock().await.clone()),
            #[cfg(feature = "storage-sqlite")]
            Backend::Sqlite(pool) => {
                let bytes: Vec<u8> = sqlx::query_scalar(
                    "SELECT state FROM provider_state WHERE singleton = 1 AND format_version = 1",
                )
                .fetch_one(pool)
                .await
                .map_err(|_| Error::internal())?;
                let root = self.root;
                tokio::task::spawn_blocking(move || State::restore(&bytes, root))
                    .await
                    .map_err(|_| Error::internal())?
            }
        }
    }

    /// Verify outside Tokio core threads, then execute against current storage.
    /// If the response is lost, replay the exact bytes; never infer non-execution.
    pub async fn execute(
        &self,
        call: SignedContractCallV1,
        now: UnixSeconds,
    ) -> Result<OperationReceipt> {
        let verified = tokio::task::spawn_blocking(move || VerifiedCall::verify(call))
            .await
            .map_err(|_| Error::internal())??;
        match self.backend.as_ref() {
            #[cfg(feature = "storage-memory")]
            Backend::Memory(state) => {
                // ponytail: one serialized runtime matches chain execution. Split
                // storage only with a reviewed cross-contract transaction design.
                let mut guard = state.lock().await;
                let current = guard.clone();
                let (next, receipt) =
                    tokio::task::spawn_blocking(move || current.execute(verified, now))
                        .await
                        .map_err(|_| Error::internal())??;
                *guard = next;
                Ok(receipt)
            }
            #[cfg(feature = "storage-sqlite")]
            Backend::Sqlite(pool) => {
                // ponytail: a whole-state row trades throughput for an auditable
                // POC transaction. Normalize rows only when state size warrants it.
                let mut tx = pool
                    .begin_with("BEGIN IMMEDIATE")
                    .await
                    .map_err(|_| Error::internal())?;
                let bytes: Vec<u8> = sqlx::query_scalar(
                    "SELECT state FROM provider_state WHERE singleton = 1 AND format_version = 1",
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| Error::internal())?;
                let root = self.root;
                let (bytes, receipt) = tokio::task::spawn_blocking(move || {
                    let current = State::restore(&bytes, root)?;
                    let (next, receipt) = current.execute(verified, now)?;
                    Ok::<_, Error>((next.encode()?, receipt))
                })
                .await
                .map_err(|_| Error::internal())??;
                let result = sqlx::query("UPDATE provider_state SET state = ? WHERE singleton = 1 AND format_version = 1")
                    .bind(bytes).execute(&mut *tx).await.map_err(|_| Error::internal())?;
                if result.rows_affected() != 1 {
                    return Err(Error::internal());
                }
                tx.commit().await.map_err(|_| Error::internal())?;
                Ok(receipt)
            }
        }
    }

    /// Public provider generation and root account, never a private key.
    pub async fn info(&self) -> Result<ProviderInfo> {
        Ok(self.read().await?.info())
    }
    /// Next nonce for a new execution; authenticated replay ignores this value.
    pub async fn nonce(&self, account: AccountId32) -> Result<AccountNonce> {
        Ok(self.read().await?.nonce(account))
    }
    /// Read-only internal snapshot; the adapter must filter private project data.
    pub async fn snapshot(&self) -> Result<CoreSnapshotV4> {
        Ok(self.read().await?.snapshot())
    }
    /// Atomic task inventory and retained global allocator from one owned state.
    pub async fn task_state(&self) -> Result<generated_contracts::provider_core::TaskStateV1> {
        let state = self.read().await?;
        tokio::task::spawn_blocking(move || state.task_state())
            .await
            .map_err(|_| Error::internal())
    }
    /// A recorded result proves finality independently of its business outcome.
    pub async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        Ok(self.read().await?.receipt(operation))
    }
    /// Exact task allocation from one owned state, without scanning live tasks.
    pub async fn task_allocation(
        &self,
        operation: OperationId,
    ) -> Result<Option<generated_contracts::provider_core::TaskAllocationV1>> {
        let state = self.read().await?;
        tokio::task::spawn_blocking(move || state.task_allocation(operation))
            .await
            .map_err(|_| Error::internal())?
    }
    /// Durable events. Limit is a transport page bound, never a matching bound.
    pub async fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        if limit == 0 || limit > 1000 {
            return Err(Error::bad("invalid_event_limit"));
        }
        Ok(self.read().await?.events(after, limit))
    }
}
