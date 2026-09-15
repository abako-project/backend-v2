use crate::crypto::{self, EncryptedSeed, Secret};
use generated_contracts::{
    AccountId32, CreateSigningJobRequest, OperationId, PAYLOAD_VERSION, PayloadHash, PrincipalId,
    SigningJobStatus, SigningJobView, Sr25519Signature, UnsignedContractCallV1, WalletId,
    WalletLifecycle, WalletView,
};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
};
use std::{
    fmt,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[derive(thiserror::Error)]
pub(crate) enum Error {
    #[error("configuration_invalid")]
    Configuration,
    #[error("secure_entropy_unavailable")]
    Entropy,
    #[error("key_unavailable")]
    KeyUnavailable,
    #[error("storage_unavailable")]
    Database(#[from] sqlx::Error),
    #[error("invalid_request")]
    InvalidRequest,
    #[error("wallet_inactive")]
    Inactive,
    #[error("expired")]
    Expired,
    #[error("not_found")]
    NotFound,
    #[error("idempotency_conflict")]
    Conflict,
    #[error("queue_full")]
    QueueFull,
    #[error("worker_unavailable")]
    Worker,
    #[error("unauthorized")]
    Unauthorized,
    #[error("clock_unavailable")]
    Clock,
}

// Even executable termination diagnostics retain only the stable, secret-free code.
impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

pub(crate) fn now() -> Result<i64, Error> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Clock)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::Clock)
}

#[derive(Clone)]
pub(crate) struct Store {
    pub(crate) pool: SqlitePool,
    master: Arc<Secret>,
}

struct StoredWallet {
    view: WalletView,
    encrypted: EncryptedSeed,
}

impl StoredWallet {
    fn read(row: &SqliteRow) -> Result<Self, Error> {
        let lifecycle = match row.try_get::<&str, _>("lifecycle")? {
            "Provisioning" => WalletLifecycle::Provisioning,
            "Active" => WalletLifecycle::Active,
            "Suspended" => WalletLifecycle::Suspended,
            "Retired" => WalletLifecycle::Retired,
            _ => return Err(Error::Configuration),
        };
        if row.try_get::<i64, _>("encryption_format_version")? != 1
            || row.try_get::<i64, _>("master_key_version")? != 1
            || row.try_get::<&str, _>("scheme")? != "sr25519"
        {
            return Err(Error::Configuration);
        }
        let nonce: Vec<u8> = row.try_get("encryption_nonce")?;
        Ok(Self {
            view: WalletView {
                wallet_id: parse(row.try_get("wallet_id")?)?,
                principal_id: row
                    .try_get::<Option<&str>, _>("principal_id")?
                    .map(parse)
                    .transpose()?,
                account_id: parse(row.try_get("account_id")?)?,
                lifecycle,
            },
            encrypted: EncryptedSeed {
                ciphertext: row.try_get("encrypted_seed")?,
                nonce: nonce.try_into().map_err(|_| Error::Configuration)?,
            },
        })
    }

    fn decrypt(&self, master: &[u8; 32]) -> Result<Secret, Error> {
        let seed = crypto::decrypt(
            master,
            &self.encrypted,
            self.view.wallet_id,
            self.view.principal_id,
        )?;
        if crypto::account(&seed)? != self.view.account_id {
            return Err(Error::KeyUnavailable);
        }
        Ok(seed)
    }
}

fn parse<T: FromStr>(value: &str) -> Result<T, Error> {
    value.parse().map_err(|_| Error::Configuration)
}

/// Only a complete, exact, policy-checked payload may reach the signing operation.
struct ValidatedJob(CreateSigningJobRequest);

impl ValidatedJob {
    fn new(
        request: CreateSigningJobRequest,
        account: AccountId32,
        time: i64,
    ) -> Result<Self, Error> {
        let call = UnsignedContractCallV1::decode_signable(&request.signable_payload)
            .map_err(|_| Error::InvalidRequest)?;
        if request.payload_version != PAYLOAD_VERSION
            || call.payload_version != request.payload_version
            || call.operation_id != request.operation_id
            || call.origin != account
            || call.expires_at != request.expires_at
            || PayloadHash::from_bytes(Sha256::digest(&request.signable_payload).into())
                != request.payload_hash
            || call.signable_bytes().map_err(|_| Error::InvalidRequest)? != request.signable_payload
        {
            return Err(Error::InvalidRequest);
        }
        let expiry = i64::try_from(request.expires_at.get()).map_err(|_| Error::InvalidRequest)?;
        if expiry <= time {
            return Err(Error::Expired);
        }
        if expiry > time.checked_add(300).ok_or(Error::Clock)? {
            return Err(Error::InvalidRequest);
        }
        Ok(Self(request))
    }

    fn sign(self, master: &[u8; 32], wallet: &StoredWallet) -> Result<Sr25519Signature, Error> {
        if wallet.view.lifecycle != WalletLifecycle::Active {
            return Err(Error::Inactive);
        }
        let seed = wallet.decrypt(master)?;
        crypto::sign(&seed, &self.0.signable_payload)
    }
}

pub(crate) struct Claim {
    request: CreateSigningJobRequest,
    token: String,
    wallet: StoredWallet,
}

impl Store {
    pub(crate) async fn open(
        database: &str,
        master: Arc<Secret>,
        root: Secret,
    ) -> Result<Self, Error> {
        let options = SqliteConnectOptions::from_str(database)?
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        // ponytail: one connection serializes short custody writes; signing runs outside it.
        // Use a larger WAL pool only if custody database contention becomes measurable.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::raw_sql(include_str!("../migrations/0001_custody.sql"))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let store = Self { pool, master };
        store.create_wallet(None, root).await?;
        // Fail readiness on the wrong key, identity substitution or damaged ciphertext.
        let rows = sqlx::query("SELECT * FROM custodial_wallets")
            .fetch_all(&store.pool)
            .await?;
        let key = store.master.clone();
        tokio::task::spawn_blocking(move || {
            for row in rows {
                StoredWallet::read(&row)?.decrypt(&key)?;
            }
            Ok::<_, Error>(())
        })
        .await
        .map_err(|_| Error::Worker)??;
        Ok(store)
    }

    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }

    pub(crate) async fn ready(&self) -> Result<(), Error> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn provision(&self, principal: PrincipalId) -> Result<WalletView, Error> {
        if let Some(row) = sqlx::query("SELECT * FROM custodial_wallets WHERE principal_id = ?")
            .bind(principal.to_string())
            .fetch_optional(&self.pool)
            .await?
        {
            return Ok(StoredWallet::read(&row)?.view);
        }
        self.create_wallet(Some(principal), Zeroizing::new(crypto::random()?))
            .await
    }

    async fn create_wallet(
        &self,
        principal: Option<PrincipalId>,
        seed: Secret,
    ) -> Result<WalletView, Error> {
        let master = self.master.clone();
        let wallet = tokio::task::spawn_blocking(move || {
            let wallet_id = WalletId::from_bytes(crypto::random()?);
            Ok::<_, Error>(StoredWallet {
                view: WalletView {
                    wallet_id,
                    principal_id: principal,
                    account_id: crypto::account(&seed)?,
                    lifecycle: WalletLifecycle::Active,
                },
                encrypted: crypto::encrypt(&master, &seed, wallet_id, principal)?,
            })
        })
        .await
        .map_err(|_| Error::Worker)??;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query("SELECT * FROM custodial_wallets WHERE principal_id IS ?")
            .bind(principal.map(|id| id.to_string()))
            .fetch_optional(&mut *transaction)
            .await?
        {
            let existing = StoredWallet::read(&row)?;
            if principal.is_none() && existing.view.account_id != wallet.view.account_id {
                return Err(Error::Configuration);
            }
            transaction.commit().await?;
            return Ok(existing.view);
        }
        let time = now()?;
        sqlx::query("INSERT INTO custodial_wallets (wallet_id, principal_id, account_id, encrypted_seed, encryption_nonce, lifecycle, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'Provisioning', ?, ?)")
            .bind(wallet.view.wallet_id.to_string()).bind(principal.map(|id| id.to_string()))
            .bind(wallet.view.account_id.to_string()).bind(wallet.encrypted.ciphertext)
            .bind(wallet.encrypted.nonce.as_slice()).bind(time).bind(time).execute(&mut *transaction).await?;
        audit(
            &mut transaction,
            None,
            Some(wallet.view.wallet_id),
            "wallet_provisioning",
            "created",
            None,
            time,
        )
        .await?;
        sqlx::query("UPDATE custodial_wallets SET lifecycle = 'Active' WHERE wallet_id = ? AND lifecycle = 'Provisioning'")
            .bind(wallet.view.wallet_id.to_string()).execute(&mut *transaction).await?;
        audit(
            &mut transaction,
            None,
            Some(wallet.view.wallet_id),
            "wallet_lifecycle",
            "active",
            None,
            time,
        )
        .await?;
        transaction.commit().await?;
        Ok(wallet.view)
    }

    pub(crate) async fn system_wallet(&self) -> Result<WalletView, Error> {
        let row = sqlx::query("SELECT * FROM custodial_wallets WHERE principal_id IS NULL")
            .fetch_one(&self.pool)
            .await?;
        Ok(StoredWallet::read(&row)?.view)
    }

    pub(crate) async fn create_job(
        &self,
        request: CreateSigningJobRequest,
    ) -> Result<SigningJobView, Error> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = sqlx::query("SELECT * FROM signing_jobs WHERE operation_id = ?")
            .bind(request.operation_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
        {
            let previous = read_request(&existing)?;
            if previous != request {
                audit(
                    &mut transaction,
                    Some(previous.operation_id),
                    Some(previous.wallet_id),
                    "signing_request",
                    "idempotency_original",
                    Some(previous.payload_hash),
                    now()?,
                )
                .await?;
                audit(
                    &mut transaction,
                    Some(request.operation_id),
                    Some(request.wallet_id),
                    "signing_request",
                    "idempotency_conflict",
                    Some(request.payload_hash),
                    now()?,
                )
                .await?;
                transaction.commit().await?;
                return Err(Error::Conflict);
            }
            transaction.commit().await?;
            return self.job(request.operation_id).await;
        }
        let wallet = sqlx::query("SELECT * FROM custodial_wallets WHERE wallet_id = ?")
            .bind(request.wallet_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(Error::NotFound)?;
        let wallet = StoredWallet::read(&wallet)?;
        let time = now()?;
        let cloned = request.clone();
        let validation = tokio::task::spawn_blocking(move || {
            ValidatedJob::new(cloned, wallet.view.account_id, time)
        })
        .await
        .map_err(|_| Error::Worker)?;
        if let Err(error) = validation {
            audit(
                &mut transaction,
                Some(request.operation_id),
                Some(request.wallet_id),
                "signing_request",
                &error.to_string(),
                Some(request.payload_hash),
                time,
            )
            .await?;
            transaction.commit().await?;
            return Err(error);
        }
        let counts = sqlx::query("SELECT COUNT(*) AS total, COALESCE(SUM(wallet_id = ?), 0) AS per_wallet FROM signing_jobs WHERE status = 'Pending'")
            .bind(request.wallet_id.to_string()).fetch_one(&mut *transaction).await?;
        if counts.try_get::<i64, _>("total")? >= 10_000
            || counts.try_get::<i64, _>("per_wallet")? >= 32
        {
            return Err(Error::QueueFull);
        }
        sqlx::query("INSERT INTO signing_jobs (operation_id, wallet_id, payload_version, signable_payload, payload_hash, expires_at, status, next_attempt_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'Pending', ?, ?, ?)")
            .bind(request.operation_id.to_string()).bind(request.wallet_id.to_string()).bind(i64::from(request.payload_version))
            .bind(&request.signable_payload).bind(request.payload_hash.as_bytes().as_slice())
            .bind(i64::try_from(request.expires_at.get()).map_err(|_| Error::InvalidRequest)?)
            .bind(time).bind(time).bind(time).execute(&mut *transaction).await?;
        audit(
            &mut transaction,
            Some(request.operation_id),
            Some(request.wallet_id),
            "signing_request",
            "pending",
            Some(request.payload_hash),
            time,
        )
        .await?;
        transaction.commit().await?;
        self.job(request.operation_id).await
    }

    pub(crate) async fn job(&self, operation: OperationId) -> Result<SigningJobView, Error> {
        let row = sqlx::query("SELECT signing_jobs.*, custodial_wallets.account_id FROM signing_jobs JOIN custodial_wallets USING(wallet_id) WHERE operation_id = ?")
            .bind(operation.to_string()).fetch_optional(&self.pool).await?.ok_or(Error::NotFound)?;
        let status = match row.try_get::<&str, _>("status")? {
            "Pending" => SigningJobStatus::Pending,
            "Signed" => SigningJobStatus::Signed,
            "Rejected" => SigningJobStatus::Rejected,
            _ => return Err(Error::Configuration),
        };
        let signature = row
            .try_get::<Option<Vec<u8>>, _>("signature")?
            .map(|bytes| {
                bytes
                    .try_into()
                    .map(Sr25519Signature::from_bytes)
                    .map_err(|_| Error::Configuration)
            })
            .transpose()?;
        Ok(SigningJobView {
            operation_id: operation,
            wallet_id: parse(row.try_get("wallet_id")?)?,
            account_id: parse(row.try_get("account_id")?)?,
            status,
            signature,
            rejection_code: row.try_get("rejection_code")?,
        })
    }

    pub(crate) async fn claim(&self, time: i64) -> Result<Option<Claim>, Error> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let recovered = sqlx::query("SELECT operation_id, wallet_id, payload_hash FROM signing_jobs WHERE status = 'Pending' AND lease_until <= ?")
            .bind(time).fetch_all(&mut *transaction).await?;
        for row in recovered {
            audit(
                &mut transaction,
                Some(parse(row.try_get("operation_id")?)?),
                Some(parse(row.try_get("wallet_id")?)?),
                "signing_lease",
                "lease_expired",
                Some(PayloadHash::from_bytes(
                    row.try_get::<Vec<u8>, _>("payload_hash")?
                        .try_into()
                        .map_err(|_| Error::Configuration)?,
                )),
                time,
            )
            .await?;
        }
        sqlx::query("UPDATE signing_jobs SET lease_token = NULL, lease_until = NULL, updated_at = ? WHERE status = 'Pending' AND lease_until <= ?")
            .bind(time).bind(time).execute(&mut *transaction).await?;
        let row = sqlx::query("SELECT * FROM signing_jobs AS candidate WHERE status = 'Pending' AND lease_token IS NULL AND next_attempt_at <= ? AND NOT EXISTS (SELECT 1 FROM signing_jobs AS earlier WHERE earlier.wallet_id = candidate.wallet_id AND earlier.status = 'Pending' AND earlier.creation_sequence < candidate.creation_sequence) ORDER BY creation_sequence LIMIT 1")
            .bind(time).fetch_optional(&mut *transaction).await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let request = read_request(&row)?;
        if row.try_get::<i64, _>("attempt_count")? >= 5 {
            sqlx::query("UPDATE signing_jobs SET status = 'Rejected', rejection_code = 'signing_attempts_exhausted', updated_at = ? WHERE operation_id = ? AND status = 'Pending' AND lease_token IS NULL")
                .bind(time).bind(request.operation_id.to_string()).execute(&mut *transaction).await?;
            audit(
                &mut transaction,
                Some(request.operation_id),
                Some(request.wallet_id),
                "signing_outcome",
                "signing_attempts_exhausted",
                Some(request.payload_hash),
                time,
            )
            .await?;
            transaction.commit().await?;
            return Ok(None);
        }
        let token = OperationId::from_bytes(crypto::random()?).to_string();
        sqlx::query("UPDATE signing_jobs SET lease_token = ?, lease_until = ?, attempt_count = attempt_count + 1, updated_at = ? WHERE operation_id = ? AND status = 'Pending' AND lease_token IS NULL")
            .bind(&token).bind(time.checked_add(30).ok_or(Error::Clock)?).bind(time)
            .bind(request.operation_id.to_string()).execute(&mut *transaction).await?;
        let wallet = StoredWallet::read(
            &sqlx::query("SELECT * FROM custodial_wallets WHERE wallet_id = ?")
                .bind(request.wallet_id.to_string())
                .fetch_one(&mut *transaction)
                .await?,
        )?;
        audit(
            &mut transaction,
            Some(request.operation_id),
            Some(request.wallet_id),
            "signing_lease",
            "claimed",
            Some(request.payload_hash),
            time,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(Claim {
            request,
            token,
            wallet,
        }))
    }

    pub(crate) async fn process_one(&self) -> Result<bool, Error> {
        let Some(claim) = self.claim(now()?).await? else {
            return Ok(false);
        };
        let master = self.master.clone();
        let (claim, outcome) = tokio::task::spawn_blocking(move || {
            let result = now()
                .and_then(|time| {
                    ValidatedJob::new(claim.request.clone(), claim.wallet.view.account_id, time)
                })
                .and_then(|validated| validated.sign(&master, &claim.wallet));
            (claim, result)
        })
        .await
        .map_err(|_| Error::Worker)?;
        self.finish(&claim, outcome, now()?).await
    }

    pub(crate) async fn finish(
        &self,
        claim: &Claim,
        outcome: Result<Sr25519Signature, Error>,
        time: i64,
    ) -> Result<bool, Error> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = sqlx::query("SELECT lifecycle FROM custodial_wallets WHERE wallet_id = ?")
            .bind(claim.request.wallet_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
        let outcome = if current.try_get::<&str, _>("lifecycle")? == "Active" {
            outcome
        } else {
            Err(Error::Inactive)
        };
        let (status, signature, rejection) = match outcome {
            Ok(signature) => ("Signed", Some(signature.as_bytes().to_vec()), None),
            Err(error) => ("Rejected", None, Some(error.to_string())),
        };
        let changed = sqlx::query("UPDATE signing_jobs SET status = ?, signature = ?, rejection_code = ?, lease_token = NULL, lease_until = NULL, updated_at = ? WHERE operation_id = ? AND status = 'Pending' AND lease_token = ? AND lease_until > ?")
            .bind(status).bind(signature).bind(&rejection).bind(time).bind(claim.request.operation_id.to_string())
            .bind(&claim.token).bind(time).execute(&mut *transaction).await?.rows_affected() == 1;
        audit(
            &mut transaction,
            Some(claim.request.operation_id),
            Some(claim.request.wallet_id),
            "signing_outcome",
            if changed {
                rejection.as_deref().unwrap_or("signed")
            } else {
                "lease_lost"
            },
            Some(claim.request.payload_hash),
            time,
        )
        .await?;
        transaction.commit().await?;
        Ok(changed)
    }

    pub(crate) async fn unauthorized(&self) -> Result<(), Error> {
        let mut connection = self.pool.acquire().await?;
        audit(
            &mut connection,
            None,
            None,
            "authorization",
            "unauthorized",
            None,
            now()?,
        )
        .await
    }

    pub(crate) async fn set_lifecycle(
        &self,
        wallet: WalletId,
        state: WalletLifecycle,
    ) -> Result<(), Error> {
        let state = match state {
            WalletLifecycle::Suspended => "Suspended",
            WalletLifecycle::Retired => "Retired",
            _ => return Err(Error::InvalidRequest),
        };
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let time = now()?;
        let row = sqlx::query("SELECT lifecycle FROM custodial_wallets WHERE wallet_id = ?")
            .bind(wallet.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(Error::NotFound)?;
        let previous = row.try_get::<&str, _>("lifecycle")?;
        if previous != state {
            if previous != "Active" {
                return Err(Error::Inactive);
            }
            sqlx::query("UPDATE custodial_wallets SET lifecycle = ?, updated_at = ? WHERE wallet_id = ? AND lifecycle = 'Active'")
                .bind(state).bind(time).bind(wallet.to_string()).execute(&mut *transaction).await?;
            audit(
                &mut transaction,
                None,
                Some(wallet),
                "wallet_lifecycle",
                state,
                None,
                time,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn metrics(&self) -> Result<String, Error> {
        let time = now()?;
        let row = sqlx::query("SELECT COALESCE(SUM(status = 'Pending'), 0) AS pending, COALESCE(SUM(status = 'Signed'), 0) AS signed, COALESCE(SUM(status = 'Rejected'), 0) AS rejected, COALESCE(SUM(attempt_count), 0) AS attempts, COALESCE(MAX(CASE WHEN status = 'Pending' THEN ? - created_at ELSE 0 END), 0) AS age FROM signing_jobs")
            .bind(time).fetch_one(&self.pool).await?;
        let recovered: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM custody_audit_records WHERE result_code = 'lease_expired'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(format!(
            "custody_pending_jobs {}\ncustody_signed_jobs_total {}\ncustody_rejected_jobs_total {}\ncustody_signing_attempts_total {}\ncustody_oldest_pending_seconds {}\ncustody_recovered_leases_total {}\n",
            row.try_get::<i64, _>("pending")?,
            row.try_get::<i64, _>("signed")?,
            row.try_get::<i64, _>("rejected")?,
            row.try_get::<i64, _>("attempts")?,
            row.try_get::<i64, _>("age")?.max(0),
            recovered
        ))
    }
}

fn read_request(row: &SqliteRow) -> Result<CreateSigningJobRequest, Error> {
    Ok(CreateSigningJobRequest {
        operation_id: parse(row.try_get("operation_id")?)?,
        wallet_id: parse(row.try_get("wallet_id")?)?,
        payload_version: u16::try_from(row.try_get::<i64, _>("payload_version")?)
            .map_err(|_| Error::Configuration)?,
        signable_payload: row.try_get("signable_payload")?,
        payload_hash: PayloadHash::from_bytes(
            row.try_get::<Vec<u8>, _>("payload_hash")?
                .try_into()
                .map_err(|_| Error::Configuration)?,
        ),
        expires_at: generated_contracts::UnixSeconds::new(
            u64::try_from(row.try_get::<i64, _>("expires_at")?)
                .map_err(|_| Error::Configuration)?,
        ),
    })
}

async fn audit(
    connection: &mut SqliteConnection,
    operation: Option<OperationId>,
    wallet: Option<WalletId>,
    kind: &str,
    code: &str,
    hash: Option<PayloadHash>,
    time: i64,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO custody_audit_records (operation_id, wallet_id, event_kind, result_code, payload_hash, occurred_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(operation.map(|id| id.to_string())).bind(wallet.map(|id| id.to_string())).bind(kind).bind(code)
        .bind(hash.map(|value| value.as_bytes().to_vec())).bind(time).execute(connection).await?;
    Ok(())
}
