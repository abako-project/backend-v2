use std::sync::Arc;

use generated_contracts::{
    AccountId32, AccountNonce, DepositView, EntityId, OperationId, OperationReceipt,
    ProviderEvents, ProviderInfo, ProviderSnapshot, SignedContractCallV1, UnixSeconds,
    WithdrawalView,
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
    pub async fn dispute(
        &self,
        id: generated_contracts::EntityId,
    ) -> Result<generated_contracts::DisputeView> {
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
        let row: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT state FROM provider_state WHERE singleton = 1 AND format_version = 1",
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| Error::internal())?;
        let bytes = tokio::task::spawn_blocking(move || {
            let mut state = match row {
                Some(bytes) => State::restore(&bytes, root)?,
                None => State::new(root)?,
            };
            state.seed();
            state.encode()
        })
        .await
        .map_err(|_| Error::internal())??;
        sqlx::query("INSERT INTO provider_state (singleton, format_version, state) VALUES (1, 1, ?) ON CONFLICT(singleton) DO UPDATE SET state = excluded.state")
            .bind(bytes).execute(&mut *tx).await.map_err(|_| Error::internal())?;
        tx.commit().await.map_err(|_| Error::internal())?;
        Ok(Self {
            backend: Arc::new(Backend::Sqlite(pool)),
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
    pub async fn snapshot(&self) -> Result<ProviderSnapshot> {
        Ok(self.read().await?.snapshot())
    }
    /// A recorded result proves finality independently of its business outcome.
    pub async fn receipt(&self, operation: OperationId) -> Result<Option<OperationReceipt>> {
        Ok(self.read().await?.receipt(operation))
    }
    /// Durable events. Limit is a transport page bound, never a matching bound.
    pub async fn events(&self, after: u64, limit: usize) -> Result<ProviderEvents> {
        if limit == 0 || limit > 1000 {
            return Err(Error::bad("invalid_event_limit"));
        }
        Ok(self.read().await?.events(after, limit))
    }
}
