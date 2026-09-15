use crate::{
    crypto,
    store::{self, Error, Store},
    web,
};
use generated_contracts::*;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{fs, future::IntoFuture, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};
use subxt_signer::sr25519;
use zeroize::Zeroizing;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn memory() -> Result<Store, Error> {
    Store::open(
        "sqlite::memory:",
        Arc::new(Zeroizing::new([31; 32])),
        Zeroizing::new([42; 32]),
    )
    .await
}

fn request(
    wallet: &WalletView,
    n: u8,
) -> Result<CreateSigningJobRequest, Box<dyn std::error::Error>> {
    let operation_id = OperationId::from_bytes([n; 16]);
    let expires_at = UnixSeconds::new(u64::try_from(store::now()?)? + 300);
    let call = UnsignedContractCallV1::new(
        ProviderInstanceId::from_bytes([3; 16]),
        operation_id,
        wallet.account_id,
        u64::from(n),
        expires_at,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Custody test".into(),
            description: "Exact authenticated bytes".into(),
        }),
    )?;
    let signable_payload = call.signable_bytes()?;
    Ok(CreateSigningJobRequest {
        operation_id,
        wallet_id: wallet.wallet_id,
        payload_version: PAYLOAD_VERSION,
        payload_hash: PayloadHash::from_bytes(Sha256::digest(&signable_payload).into()),
        signable_payload,
        expires_at,
    })
}

#[test]
fn ciphertext_is_bound_to_wallet_principal_key_and_authentication_tag() -> TestResult {
    let wallet = WalletId::from_bytes([1; 16]);
    let principal = Some(PrincipalId::from_bytes([2; 16]));
    let seed = [3; 32];
    let master = [4; 32];
    let mut encrypted = crypto::encrypt(&master, &seed, wallet, principal)?;
    assert!(
        !encrypted
            .ciphertext
            .windows(32)
            .any(|window| window == seed)
    );
    assert_eq!(
        *crypto::decrypt(&master, &encrypted, wallet, principal)?,
        seed
    );
    assert!(crypto::decrypt(&[5; 32], &encrypted, wallet, principal).is_err());
    assert!(
        crypto::decrypt(
            &master,
            &encrypted,
            WalletId::from_bytes([2; 16]),
            principal
        )
        .is_err()
    );
    assert!(crypto::decrypt(&master, &encrypted, wallet, None).is_err());
    encrypted.ciphertext[0] ^= 1;
    assert!(crypto::decrypt(&master, &encrypted, wallet, principal).is_err());
    assert_eq!(format!("{:?}", Error::KeyUnavailable), "key_unavailable");
    Ok(())
}

#[tokio::test]
async fn provisioning_and_signed_jobs_are_durable_idempotent_and_exact() -> TestResult {
    let store = memory().await?;
    let principal = PrincipalId::from_bytes([7; 16]);
    let (first, second) = tokio::join!(store.provision(principal), store.provision(principal));
    let wallet = first?;
    assert_eq!(wallet, second?);
    assert_ne!(wallet.account_id, store.system_wallet().await?.account_id);
    assert_eq!(store.system_wallet().await?.principal_id, None);
    let request = request(&wallet, 1)?;
    assert_eq!(
        store.create_job(request.clone()).await?.status,
        SigningJobStatus::Pending
    );
    assert_eq!(
        store.create_job(request.clone()).await?.status,
        SigningJobStatus::Pending
    );
    assert!(store.process_one().await?);
    let signed = store.job(request.operation_id).await?;
    assert_eq!(signed.status, SigningJobStatus::Signed);
    let signature = signed.signature.ok_or("signature missing")?;
    assert!(sr25519::verify(
        &sr25519::Signature(signature.into_bytes()),
        &request.signable_payload,
        &sr25519::PublicKey(wallet.account_id.into_bytes())
    ));
    assert_eq!(store.create_job(request.clone()).await?, signed);
    for index in 0..request.signable_payload.len() {
        let mut changed = request.signable_payload.clone();
        changed[index] ^= 1;
        assert!(!sr25519::verify(
            &sr25519::Signature(signature.into_bytes()),
            &changed,
            &sr25519::PublicKey(wallet.account_id.into_bytes())
        ));
    }
    let mut collision = request.clone();
    collision.signable_payload[0] ^= 1;
    assert!(matches!(
        store.create_job(collision).await,
        Err(Error::Conflict)
    ));
    assert_eq!(store.job(request.operation_id).await?, signed);
    let mut expired = request;
    expired.expires_at = UnixSeconds::new(1);
    assert!(matches!(
        store.create_job(expired).await,
        Err(Error::Conflict)
    ));
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM signing_jobs")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(rows, 1);
    Ok(())
}

#[tokio::test]
async fn reject_payload_version_hash_origin_expiry_and_trailing_bytes() -> TestResult {
    let store = memory().await?;
    let wallet = store.provision(PrincipalId::from_bytes([7; 16])).await?;
    let base = request(&wallet, 1)?;
    let mut bad_version = base.clone();
    bad_version.payload_version = 2;
    let mut bad_hash = base.clone();
    bad_hash.payload_hash = PayloadHash::from_bytes([0; 32]);
    let mut bad_id = base.clone();
    bad_id.operation_id = OperationId::from_bytes([9; 16]);
    let mut trailing = base.clone();
    trailing.signable_payload.push(0);
    let mut wrong_wallet = base.clone();
    wrong_wallet.wallet_id = store.system_wallet().await?.wallet_id;
    for request in [bad_version, bad_hash, bad_id, trailing, wrong_wallet] {
        assert!(matches!(
            store.create_job(request).await,
            Err(Error::InvalidRequest)
        ));
    }
    for expiry in [1, u64::try_from(store::now()?)? + 3600] {
        let mut invalid = base.clone();
        let mut call = UnsignedContractCallV1::decode_signable(&invalid.signable_payload)?;
        call.expires_at = UnixSeconds::new(expiry);
        invalid.expires_at = call.expires_at;
        invalid.signable_payload = call.signable_bytes()?;
        invalid.payload_hash =
            PayloadHash::from_bytes(Sha256::digest(&invalid.signable_payload).into());
        assert!(store.create_job(invalid).await.is_err());
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM signing_jobs")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(rows, 0);
    Ok(())
}

#[tokio::test]
async fn leases_preserve_wallet_order_allow_other_wallets_and_reject_stale_completion() -> TestResult
{
    let store = memory().await?;
    let wallet = store.provision(PrincipalId::from_bytes([7; 16])).await?;
    let other = store.provision(PrincipalId::from_bytes([8; 16])).await?;
    let first = request(&wallet, 250)?; // insertion sequence, not random operation ID, orders work.
    let second = request(&wallet, 1)?;
    let unrelated = request(&other, 2)?;
    for job in [&first, &second, &unrelated] {
        store.create_job(job.clone()).await?;
    }
    let time = store::now()?;
    let original = store.claim(time).await?.ok_or("first claim missing")?;
    let other_claim = store.claim(time).await?.ok_or("other wallet blocked")?;
    assert!(store.claim(time).await?.is_none());
    assert!(
        store
            .finish(&other_claim, Err(Error::Inactive), time)
            .await?
    );
    assert_eq!(
        store.job(unrelated.operation_id).await?.status,
        SigningJobStatus::Rejected
    );
    let recovered = store
        .claim(time + 31)
        .await?
        .ok_or("expired lease not recovered")?;
    assert!(
        !store
            .finish(&original, Err(Error::Inactive), time + 31)
            .await?
    );
    assert_eq!(
        store.job(first.operation_id).await?.status,
        SigningJobStatus::Pending
    );
    assert!(
        store
            .finish(&recovered, Err(Error::Inactive), time + 31)
            .await?
    );
    let next = store.claim(time + 31).await?.ok_or("second job blocked")?;
    assert!(store.finish(&next, Err(Error::Inactive), time + 31).await?);
    assert_eq!(
        store.job(second.operation_id).await?.status,
        SigningJobStatus::Rejected
    );
    assert!(
        !store
            .finish(&recovered, Err(Error::Inactive), time + 31)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn inactive_wallets_and_exhausted_claims_never_return_a_signature() -> TestResult {
    for state in ["Suspended", "Retired"] {
        let store = memory().await?;
        let wallet = store.provision(PrincipalId::from_bytes([7; 16])).await?;
        let job = request(&wallet, 1)?;
        store.create_job(job.clone()).await?;
        let lifecycle = if state == "Suspended" {
            WalletLifecycle::Suspended
        } else {
            WalletLifecycle::Retired
        };
        store.set_lifecycle(wallet.wallet_id, lifecycle).await?;
        store.set_lifecycle(wallet.wallet_id, lifecycle).await?;
        assert!(
            store
                .set_lifecycle(wallet.wallet_id, WalletLifecycle::Active)
                .await
                .is_err()
        );
        store.process_one().await?;
        let result = store.job(job.operation_id).await?;
        assert_eq!(result.status, SigningJobStatus::Rejected);
        assert_eq!(result.rejection_code.as_deref(), Some("wallet_inactive"));
        assert!(result.signature.is_none());
    }
    let store = memory().await?;
    let wallet = store.system_wallet().await?;
    let job = request(&wallet, 1)?;
    store.create_job(job.clone()).await?;
    let time = store::now()?;
    for attempt in 0..5 {
        assert!(store.claim(time + attempt * 31).await?.is_some());
    }
    assert!(store.claim(time + 155).await?.is_none());
    assert_eq!(
        store.job(job.operation_id).await?.rejection_code.as_deref(),
        Some("signing_attempts_exhausted")
    );
    Ok(())
}

#[tokio::test]
async fn queue_bounds_do_not_block_exact_retries() -> TestResult {
    let store = memory().await?;
    let wallet = store.system_wallet().await?;
    let first = request(&wallet, 0)?;
    store.create_job(first.clone()).await?;
    for id in 1..32 {
        store.create_job(request(&wallet, id)?).await?;
    }
    assert!(store.create_job(first).await.is_ok());
    assert!(matches!(
        store.create_job(request(&wallet, 32)?).await,
        Err(Error::QueueFull)
    ));
    Ok(())
}

fn temp_path() -> Result<PathBuf, Error> {
    Ok(std::env::temp_dir().join(format!(
        "kunveno-custody-test-{}",
        OperationId::from_bytes(crypto::random()?)
    )))
}

#[test]
fn dev_secrets_are_private_create_new_valid_and_never_rotated_implicitly() -> TestResult {
    let directory = temp_path()?;
    crypto::init_dev_secrets(&directory)?;
    assert_eq!(
        fs::metadata(&directory)?.permissions().mode() & 0o777,
        0o700
    );
    let master_path = directory.join("master-key.hex");
    let seed_path = directory.join("root-seed.hex");
    let master = crypto::read_secret(master_path.to_str().ok_or("path")?)?;
    let seed = crypto::read_secret(seed_path.to_str().ok_or("path")?)?;
    assert_ne!(*master, *seed);
    assert_eq!(
        crypto::account(&seed)?,
        fs::read_to_string(directory.join("root-account.hex"))?
            .trim()
            .parse()?
    );
    let token = crypto::read_token(directory.join("service-token").to_str().ok_or("path")?)?;
    assert_eq!(token.len(), 64);
    for entry in fs::read_dir(&directory)? {
        assert_eq!(entry?.metadata()?.permissions().mode() & 0o777, 0o600);
    }
    assert!(crypto::init_dev_secrets(&directory).is_err());
    assert_eq!(
        *crypto::read_secret(master_path.to_str().ok_or("path")?)?,
        *master
    );
    fs::set_permissions(&master_path, fs::Permissions::from_mode(0o644))?;
    assert!(crypto::read_secret(master_path.to_str().ok_or("path")?).is_err());
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn sqlite_restart_keeps_wallet_ciphertext_jobs_and_signatures() -> TestResult {
    let directory = temp_path()?;
    crypto::init_dev_secrets(&directory)?;
    let database = format!("sqlite://{}", directory.join("custody.sqlite").display());
    let master = Arc::new(Zeroizing::new([31; 32]));
    let store = Store::open(&database, master.clone(), Zeroizing::new([42; 32])).await?;
    let principal = PrincipalId::from_bytes([7; 16]);
    let wallet = store.provision(principal).await?;
    let pending = request(&wallet, 1)?;
    store.create_job(pending.clone()).await?;
    store.close().await;
    assert!(
        Store::open(
            &database,
            Arc::new(Zeroizing::new([32; 32])),
            Zeroizing::new([42; 32])
        )
        .await
        .is_err()
    );
    assert!(
        Store::open(&database, master.clone(), Zeroizing::new([43; 32]))
            .await
            .is_err()
    );
    let store = Store::open(&database, master.clone(), Zeroizing::new([42; 32])).await?;
    assert_eq!(store.provision(principal).await?, wallet);
    assert_eq!(
        store.job(pending.operation_id).await?.status,
        SigningJobStatus::Pending
    );
    store.process_one().await?;
    let signed = store.job(pending.operation_id).await?;
    store.close().await;
    let store = Store::open(&database, master, Zeroizing::new([42; 32])).await?;
    assert_eq!(store.create_job(pending).await?, signed);
    store.close().await;
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
async fn internal_http_authentication_async_status_and_redacted_errors() -> TestResult {
    let store = memory().await?;
    let token = "custody-tests-only-012345678901234567890";
    let state = web::AppState::new(store.clone(), Zeroizing::new(token.to_owned()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        axum::serve(listener, web::router(state))
            .with_graceful_shutdown(async {
                let _closed = stopped.await;
            })
            .into_future(),
    );
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    assert_eq!(
        http.get(format!("{url}/health")).send().await?.status(),
        204
    );
    for credential in [None, Some("wrong-token")] {
        let mut request = http.get(format!("{url}/internal/system-wallet"));
        if let Some(credential) = credential {
            request = request.bearer_auth(credential);
        }
        let response = request.send().await?;
        assert_eq!(response.status(), 401);
        let body = response.text().await?;
        assert!(!body.contains(token));
    }
    let response = http
        .post(format!("{url}/internal/wallets"))
        .bearer_auth(token)
        .json(&ProvisionWalletRequest {
            principal_id: PrincipalId::from_bytes([7; 16]),
        })
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let wallet: WalletView = response.json().await?;
    let payload = request(&wallet, 1)?;
    let response = http
        .post(format!("{url}/internal/signing-jobs"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await?;
    assert_eq!(response.status(), 202);
    assert_eq!(
        response.json::<SigningJobView>().await?.status,
        SigningJobStatus::Pending
    );
    store.process_one().await?;
    let result: SigningJobView = http
        .get(format!(
            "{url}/internal/signing-jobs/{}",
            payload.operation_id
        ))
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(result.status, SigningJobStatus::Signed);
    let denied = sqlx::query(
        "SELECT COUNT(*) AS count FROM custody_audit_records WHERE result_code = 'unauthorized'",
    )
    .fetch_one(&store.pool)
    .await?
    .try_get::<i64, _>("count")?;
    assert_eq!(denied, 2);
    let metrics = http
        .get(format!("{url}/internal/metrics"))
        .bearer_auth(token)
        .send()
        .await?
        .text()
        .await?;
    assert!(metrics.contains("custody_signed_jobs_total 1\n"));
    assert!(!metrics.contains(token));
    assert!(!metrics.contains(&wallet.account_id.to_string()));
    shutdown.send(()).map_err(|()| "server stopped")?;
    server.await??;
    Ok(())
}
