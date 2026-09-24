use crate::{
    auth::Session,
    state::{App, Error, now, parse, payload_hash, random, token},
};
use generated_contracts::{
    AccountId32, AccountNonce, CreateSigningJobRequest, MAX_SIGNABLE_BYTES, OperationId,
    OperationReceipt, OperationRef, OperationView, PAYLOAD_VERSION, PreparedCall, ProjectView,
    ProviderCommand, ProviderInfo, ProviderOperationStatus, SignedContractCallV1, SigningJobStatus,
    SigningJobView, UnixSeconds, UnsignedContractCallV1, WalletId, WalletLifecycle, WalletView,
    WorkerMode,
};
use parity_scale_codec::Encode;
use sqlx::{Row, postgres::PgRow};
use std::{sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};

pub(crate) fn visible(project: &ProjectView, account: AccountId32) -> bool {
    project.client == account
        || project.coordinator == account
        || project
            .proposals
            .iter()
            .flat_map(|p| &p.milestones)
            .any(|m| {
                m.assignments.iter().any(|a| a.worker == account)
                    || m.task_storage
                        .tasks
                        .iter()
                        .any(|t| t.task.assignees.contains(&account))
            })
}
fn privileged(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::PromoteCoordinator(_)
            | ProviderCommand::UpsertCatalogEntry(_)
            | ProviderCommand::DeleteCatalogEntry(_)
            | ProviderCommand::FundAccount(_)
            | ProviderCommand::ConfirmDeposit { .. }
            | ProviderCommand::SetScorePolicy(_)
    )
}

pub(crate) async fn authorize(
    app: &App,
    session: &Session,
    command: &ProviderCommand,
) -> Result<(), Error> {
    if privileged(command) {
        return if session.view.is_admin {
            Ok(())
        } else {
            Err(Error::Forbidden)
        };
    }
    let account = session.view.account_id;
    match command {
        ProviderCommand::CreateDeposit(_) | ProviderCommand::CreateWithdrawal(_) => {
            return Ok(());
        }
        ProviderCommand::CancelWithdrawal { withdrawal_id } => {
            if !session.view.is_admin {
                crate::bramp::read_withdrawal(app, account, *withdrawal_id).await?;
            }
            return Ok(());
        }
        _ => {}
    }
    let snapshot = app.snapshot().await?;
    if let Some(id) = command.project_id() {
        let project = snapshot
            .projects
            .iter()
            .find(|p| p.project_id == id)
            .ok_or(Error::NotFound)?;
        let allowed = match command {
            ProviderCommand::QuotePlanning { .. }
            | ProviderCommand::CreateProposal { .. }
            | ProviderCommand::UpdateProposal { .. }
            | ProviderCommand::DeleteProposal { .. }
            | ProviderCommand::SubmitProposal { .. }
            | ProviderCommand::CreateTask { .. }
            | ProviderCommand::EditTask { .. }
            | ProviderCommand::RequestMilestoneCompletion { .. } => project.coordinator == account,
            ProviderCommand::AcceptPlanningQuote { .. }
            | ProviderCommand::AcceptPlanningDelivery { .. }
            | ProviderCommand::ApproveExecution { .. }
            | ProviderCommand::RequestProposalChanges { .. }
            | ProviderCommand::AcceptMilestoneCompletion { .. }
            | ProviderCommand::RejectMilestoneCompletion { .. } => project.client == account,
            ProviderCommand::UpdateTaskProgress {
                task_storage_id,
                task_id,
                ..
            } => {
                project.client != account
                    && project
                        .proposals
                        .iter()
                        .flat_map(|p| &p.milestones)
                        .find(|m| m.task_storage.task_storage_id == *task_storage_id)
                        .and_then(|m| m.task_storage.tasks.iter().find(|t| t.task_id == *task_id))
                        .is_some_and(|task| task.task.assignees.contains(&account))
            }
            ProviderCommand::CancelProject { .. }
            | ProviderCommand::DisputePlanning { .. }
            | ProviderCommand::OpenDispute(_) => {
                project.client == account || project.coordinator == account
            }
            ProviderCommand::RespondDispute { dispute_id, .. } => {
                crate::disputes::read(app, *dispute_id)
                    .await?
                    .dispute
                    .counterparty
                    == account
            }
            _ => false,
        };
        return if allowed {
            Ok(())
        } else {
            Err(Error::Forbidden)
        };
    }
    match command {
        ProviderCommand::RegisterWorker(_) | ProviderCommand::CreateProject(_) => Ok(()),
        ProviderCommand::UpdateQualifications(_) | ProviderCommand::SetCalendar(_) => {
            if snapshot.workers.iter().any(|w| w.account == account) {
                Ok(())
            } else {
                Err(Error::Forbidden)
            }
        }
        ProviderCommand::SetWorkerMode(request) => {
            if snapshot.workers.iter().any(|w| {
                w.account == account
                    && (request.mode == WorkerMode::Worker || w.coordinator_eligible)
            }) {
                Ok(())
            } else {
                Err(Error::Forbidden)
            }
        }
        _ => Err(Error::Forbidden),
    }
}

pub(crate) async fn enqueue(
    app: &App,
    session: &Session,
    command: ProviderCommand,
    key: Option<OperationId>,
) -> Result<OperationRef, Error> {
    command.validate().map_err(|_| Error::Invalid)?;
    let command_bytes = command.encode();
    if command_bytes.len() > MAX_SIGNABLE_BYTES - 128 {
        return Err(Error::Invalid);
    }
    let operation_id = match key {
        Some(id) => id,
        None => OperationId::from_bytes(random()?),
    };
    if let Some(row) = sqlx::query(
        "SELECT principal_id, command_bytes, status FROM operations WHERE operation_id = $1",
    )
    .bind(operation_id.to_string())
    .fetch_optional(&app.db)
    .await?
    {
        return existing(&row, session, &command_bytes, operation_id);
    }
    if let Err(error) = authorize(app, session, &command).await {
        app.audit(
            Some(&session.view.principal_id.to_string()),
            Some(&operation_id.to_string()),
            "operation_denied",
            error.status_code().1,
        )
        .await?;
        return Err(error);
    }
    let (wallet, account) = if privileged(&command)
        || (session.view.is_admin && matches!(command, ProviderCommand::CancelWithdrawal { .. }))
    {
        let system: WalletView = app
            .get(&app.config.custody_url, "/internal/system-wallet")
            .await?;
        let info: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
        if system.principal_id.is_some()
            || system.lifecycle != WalletLifecycle::Active
            || system.account_id != info.root_account
        {
            return Err(Error::Dependency);
        }
        (system.wallet_id, system.account_id)
    } else {
        (session.wallet, session.view.account_id)
    };
    let time = now()?;
    let mut tx = app.db.begin().await?;
    // One database-wide enqueue lock preserves both queue limits across replicas.
    sqlx::query("SELECT pg_advisory_xact_lock(621007)")
        .execute(&mut *tx)
        .await?;
    if let Some(row) = sqlx::query(
        "SELECT principal_id, command_bytes, status FROM operations WHERE operation_id = $1",
    )
    .bind(operation_id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    {
        return existing(&row, session, &command_bytes, operation_id);
    }
    let count = sqlx::query("SELECT count(*) AS total, count(*) FILTER (WHERE wallet_id = $1) AS wallet FROM operations WHERE status NOT IN ('Finalized', 'Rejected', 'Expired')")
        .bind(wallet.to_string()).fetch_one(&mut *tx).await?;
    if count.try_get::<i64, _>("total")? >= 10_000 || count.try_get::<i64, _>("wallet")? >= 32 {
        return Err(Error::Capacity);
    }
    sqlx::query("INSERT INTO operations(operation_id, principal_id, wallet_id, account_id, command_json, command_bytes, status, next_attempt_at, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, 'AwaitingSignature', $7, $8, $9)")
        .bind(operation_id.to_string()).bind(session.view.principal_id.to_string()).bind(wallet.to_string()).bind(account.to_string())
        .bind(serde_json::to_string(&command)?).bind(command_bytes).bind(time).bind(time + 300).bind(time).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_records(principal_id, operation_id, event_kind, result_code, occurred_at) VALUES ($1, $2, 'operation_authorized', 'ok', $3)")
        .bind(session.view.principal_id.to_string()).bind(operation_id.to_string()).bind(time).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(OperationRef {
        operation_id,
        status: ProviderOperationStatus::AwaitingSignature,
    })
}
fn existing(
    row: &PgRow,
    session: &Session,
    command: &[u8],
    operation_id: OperationId,
) -> Result<OperationRef, Error> {
    if row.try_get::<&str, _>("principal_id")? != session.view.principal_id.to_string()
        || row.try_get::<Vec<u8>, _>("command_bytes")? != command
    {
        return Err(Error::Conflict("idempotency_conflict"));
    }
    Ok(OperationRef {
        operation_id,
        status: status(row.try_get("status")?)?,
    })
}
fn status(name: &str) -> Result<ProviderOperationStatus, Error> {
    Ok(serde_json::from_value(serde_json::Value::String(
        name.to_owned(),
    ))?)
}
fn name(status: ProviderOperationStatus) -> &'static str {
    match status {
        ProviderOperationStatus::AwaitingSignature => "AwaitingSignature",
        ProviderOperationStatus::ReadyToSubmit => "ReadyToSubmit",
        ProviderOperationStatus::Submitted => "Submitted",
        ProviderOperationStatus::OutcomeUnknown => "OutcomeUnknown",
        ProviderOperationStatus::Finalized => "Finalized",
        ProviderOperationStatus::Rejected => "Rejected",
        ProviderOperationStatus::Expired => "Expired",
    }
}
pub(crate) async fn read(
    app: &App,
    session: &Session,
    id: OperationId,
) -> Result<OperationView, Error> {
    let row = sqlx::query("SELECT status, receipt_json, error_code FROM operations WHERE operation_id = $1 AND principal_id = $2")
        .bind(id.to_string()).bind(session.view.principal_id.to_string()).fetch_optional(&app.db).await?.ok_or(Error::NotFound)?;
    Ok(OperationView {
        operation_id: id,
        status: status(row.try_get("status")?)?,
        receipt: row
            .try_get::<Option<&str>, _>("receipt_json")?
            .map(serde_json::from_str)
            .transpose()?,
        error_code: row.try_get("error_code")?,
    })
}

pub(crate) struct Job {
    id: OperationId,
    wallet: WalletId,
    account: AccountId32,
    command: ProviderCommand,
    status: ProviderOperationStatus,
    payload: Option<Vec<u8>>,
    signed: Option<String>,
    instance: Option<String>,
    receipt: Option<String>,
    error: Option<String>,
    submitted: bool,
    attempts: i64,
    next: i64,
    expires: i64,
    lease: String,
}
impl Job {
    fn from_row(row: &PgRow, lease: String) -> Result<Self, Error> {
        Ok(Self {
            id: parse(row.try_get("operation_id")?)?,
            wallet: parse(row.try_get("wallet_id")?)?,
            account: parse(row.try_get("account_id")?)?,
            command: serde_json::from_str(row.try_get("command_json")?)?,
            status: status(row.try_get("status")?)?,
            payload: row.try_get("signable_payload")?,
            signed: row.try_get("signed_json")?,
            instance: row.try_get("provider_instance_id")?,
            receipt: row.try_get("receipt_json")?,
            error: row.try_get("error_code")?,
            submitted: row.try_get("possibly_submitted")?,
            attempts: row.try_get("attempt_count")?,
            next: row.try_get("next_attempt_at")?,
            expires: row.try_get("expires_at")?,
            lease,
        })
    }
    fn terminal(&mut self, status: ProviderOperationStatus, code: &str) {
        self.status = status;
        self.error = Some(code.to_owned());
    }
    fn retry(&mut self, time: i64, code: &str) {
        self.attempts = self.attempts.saturating_add(1).min(5);
        self.next = time + (1_i64 << self.attempts.min(5)).min(30);
        self.error = Some(code.into());
        if self.submitted {
            self.status = ProviderOperationStatus::OutcomeUnknown;
        } else if self.attempts >= 5 {
            self.status = ProviderOperationStatus::Rejected;
        }
    }
}

pub(crate) async fn claim(app: &App) -> Result<Option<Job>, Error> {
    let time = now()?;
    let lease = token()?.to_string();
    let mut tx = app.db.begin().await?;
    let row = sqlx::query("SELECT * FROM operations o WHERE o.status NOT IN ('Finalized', 'Rejected', 'Expired') AND (o.lease_token IS NULL OR o.lease_until <= $1) AND o.next_attempt_at <= $1 AND NOT EXISTS (SELECT 1 FROM operations previous WHERE previous.wallet_id = o.wallet_id AND previous.creation_sequence < o.creation_sequence AND previous.status NOT IN ('Finalized', 'Rejected', 'Expired')) ORDER BY o.creation_sequence LIMIT 1 FOR UPDATE OF o SKIP LOCKED")
        .bind(time).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    if row.try_get::<Option<&str>, _>("lease_token")?.is_some() {
        sqlx::query("INSERT INTO audit_records(operation_id, event_kind, result_code, occurred_at) VALUES ($1, 'lease_recovery', 'expired', $2)")
            .bind(row.try_get::<&str, _>("operation_id")?).bind(time).execute(&mut *tx).await?;
    }
    let job = Job::from_row(&row, lease)?;
    sqlx::query("UPDATE operations SET lease_token = $1, lease_until = $2 WHERE operation_id = $3")
        .bind(&job.lease)
        .bind(time + 30)
        .bind(job.id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(job))
}
async fn save(app: &App, job: &Job, release: bool) -> Result<(), Error> {
    let time = now()?;
    let mut tx = app.db.begin().await?;
    let result = sqlx::query("UPDATE operations SET status = $1, signable_payload = $2, signed_json = $3, provider_instance_id = $4, receipt_json = $5, error_code = $6, possibly_submitted = $7, attempt_count = $8, next_attempt_at = $9, lease_token = CASE WHEN $10 THEN NULL ELSE lease_token END, lease_until = CASE WHEN $11 THEN NULL ELSE lease_until END WHERE operation_id = $12 AND lease_token = $13 AND lease_until > $14")
        .bind(name(job.status)).bind(&job.payload).bind(&job.signed).bind(&job.instance).bind(&job.receipt).bind(&job.error)
        .bind(job.submitted).bind(job.attempts).bind(job.next).bind(release).bind(release).bind(job.id.to_string()).bind(&job.lease).bind(time).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(Error::Conflict("lease_lost"));
    }
    sqlx::query("INSERT INTO audit_records(operation_id, event_kind, result_code, occurred_at) VALUES ($1, 'operation_stage', $2, $3)")
        .bind(job.id.to_string()).bind(name(job.status)).bind(time).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn process(app: &App, mut job: Job) -> Result<(), Error> {
    let time = now()?;
    let outcome = advance(app, &mut job, time).await;
    if let Err(error) = outcome {
        if matches!(error, Error::Conflict("lease_lost")) {
            return Ok(());
        }
        tracing::warn!(operation_id = %job.id, stage = ?job.status, code = error.status_code().1, "operation stage failed");
        job.retry(time, error.status_code().1);
    }
    match save(app, &job, true).await {
        Err(Error::Conflict("lease_lost")) => Ok(()),
        result => result,
    }
}
#[allow(clippy::too_many_lines)] // Keep the ordered transport stages and uncertainty guards together.
async fn advance(app: &App, job: &mut Job, time: i64) -> Result<(), Error> {
    if job.payload.is_none() {
        if time >= job.expires {
            job.terminal(ProviderOperationStatus::Expired, "expired");
            return Ok(());
        }
        let info: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
        if info.payload_version != PAYLOAD_VERSION {
            job.terminal(ProviderOperationStatus::Rejected, "unsupported_provider");
            return Ok(());
        }
        let nonce: AccountNonce = app
            .get(
                &app.config.provider_url,
                &format!("/internal/accounts/{}/nonce", job.account),
            )
            .await?;
        if nonce.account != job.account {
            return Err(Error::Dependency);
        }
        let call = UnsignedContractCallV1::new(
            info.provider_instance_id,
            job.id,
            job.account,
            nonce.nonce,
            UnixSeconds::new(u64::try_from(job.expires).map_err(|_| Error::Internal)?),
            job.command.clone(),
        )
        .map_err(|_| Error::Invalid)?;
        let prepared = PreparedCall::new(call).map_err(|_| Error::Invalid)?;
        job.payload = Some(prepared.bytes().to_vec());
        job.instance = Some(info.provider_instance_id.to_string());
        job.attempts = 0;
        job.next = time;
        job.error = None;
        return Ok(());
    }
    let call =
        UnsignedContractCallV1::decode_signable(job.payload.as_deref().ok_or(Error::Internal)?)
            .map_err(|_| Error::Internal)?;
    if call.operation_id != job.id
        || call.origin != job.account
        || call.command != job.command
        || Some(call.provider_instance_id.to_string()) != job.instance
    {
        return Err(Error::Internal);
    }
    if job.signed.is_none() {
        if time >= job.expires {
            job.terminal(ProviderOperationStatus::Expired, "expired");
            return Ok(());
        }
        let bytes = job.payload.as_ref().ok_or(Error::Internal)?;
        let signed: SigningJobView = match app
            .get(
                &app.config.custody_url,
                &format!("/internal/signing-jobs/{}", job.id),
            )
            .await
        {
            Ok(job) => job,
            Err(Error::NotFound) => {
                app.post(
                    &app.config.custody_url,
                    "/internal/signing-jobs",
                    &CreateSigningJobRequest {
                        operation_id: job.id,
                        wallet_id: job.wallet,
                        payload_version: PAYLOAD_VERSION,
                        signable_payload: bytes.clone(),
                        payload_hash: payload_hash(bytes),
                        expires_at: call.expires_at,
                    },
                )
                .await?
            }
            Err(error) => return Err(error),
        };
        if signed.operation_id != job.id
            || signed.wallet_id != job.wallet
            || signed.account_id != job.account
        {
            return Err(Error::Dependency);
        }
        match signed.status {
            SigningJobStatus::Pending => {
                job.next = time + 1;
            }
            SigningJobStatus::Rejected => {
                job.terminal(ProviderOperationStatus::Rejected, "signing_rejected");
            }
            SigningJobStatus::Signed => {
                let signature = signed.signature.ok_or(Error::Dependency)?;
                let prepared = PreparedCall::new(call).map_err(|_| Error::Internal)?;
                job.signed = Some(serde_json::to_string(&prepared.with_signature(signature))?);
                job.status = ProviderOperationStatus::ReadyToSubmit;
                job.attempts = 0;
                job.next = time;
                job.error = None;
            }
        }
        return Ok(());
    }
    let current: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
    if current.provider_instance_id != call.provider_instance_id {
        job.terminal(
            if job.submitted {
                ProviderOperationStatus::OutcomeUnknown
            } else {
                ProviderOperationStatus::Rejected
            },
            "provider_instance_changed",
        );
        job.next = time + 30;
        return Ok(());
    }
    if job.submitted {
        match app
            .get::<OperationReceipt>(
                &app.config.provider_url,
                &format!("/internal/receipts/{}", job.id),
            )
            .await
        {
            Ok(receipt) => {
                return finalize(job, &call, &receipt);
            }
            Err(Error::NotFound) => {}
            Err(error) => return Err(error),
        }
        if time >= job.expires || job.attempts >= 5 {
            job.terminal(
                ProviderOperationStatus::OutcomeUnknown,
                "reconciliation_pending",
            );
            job.next = time + 30;
            return Ok(());
        }
    } else if time >= job.expires {
        job.terminal(ProviderOperationStatus::Expired, "expired");
        return Ok(());
    }
    let signed: SignedContractCallV1 =
        serde_json::from_str(job.signed.as_deref().ok_or(Error::Internal)?)?;
    if signed.call != call {
        return Err(Error::Internal);
    }
    let prior_submission = job.submitted;
    // This durable marker precedes sending any bytes. A crash here is uncertain, never rejected.
    job.submitted = true;
    job.status = ProviderOperationStatus::Submitted;
    save(app, job, false).await?;
    let response = app
        .internal(
            reqwest::Method::POST,
            &app.config.provider_url,
            "/internal/contracts/call",
        )
        .json(&signed)
        .send()
        .await
        .map_err(|_| Error::Dependency)?;
    if response.status().is_success() {
        return finalize(job, &call, &crate::state::decode_response(response).await?);
    }
    if !prior_submission
        && response.status().is_client_error()
        && !matches!(response.status().as_u16(), 408 | 429)
    {
        // A first, explicit HTTP rejection establishes no execution of this immutable call.
        job.terminal(ProviderOperationStatus::Rejected, "provider_rejected");
        return Ok(());
    }
    Err(Error::Dependency)
}
fn finalize(
    job: &mut Job,
    call: &UnsignedContractCallV1,
    receipt: &OperationReceipt,
) -> Result<(), Error> {
    if receipt.operation_id != call.operation_id
        || receipt.origin != call.origin
        || receipt.nonce != call.nonce
        || receipt.provider_instance_id != call.provider_instance_id
    {
        return Err(Error::Dependency);
    }
    job.receipt = Some(serde_json::to_string(&receipt)?);
    job.status = ProviderOperationStatus::Finalized;
    job.error = None;
    job.attempts = 0;
    Ok(())
}

pub(crate) async fn run(app: Arc<App>, mut stop: watch::Receiver<bool>) -> Result<(), Error> {
    let mut tasks = JoinSet::new();
    loop {
        if *stop.borrow() {
            break;
        }
        while tasks.len() < 8 {
            match claim(&app).await? {
                Some(job) => {
                    let worker = app.clone();
                    tasks.spawn(async move { process(&worker, job).await });
                }
                None => break,
            }
        }
        tokio::select! {
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(result) = result { result??; }
            }
            changed = stop.changed() => { if changed.is_err() { break; } }
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    Ok(())
}
