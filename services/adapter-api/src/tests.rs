#![allow(clippy::too_many_lines)]

use crate::{
    auth::{self, Session},
    http, notifications, operations,
    state::{App, Config, Error, hash, now, parse, payload_hash},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use generated_contracts::*;
use sqlx::Row;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use zeroize::Zeroizing;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const PASSWORD: &str = "adapter-test-password-123";

struct Server {
    url: String,
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), std::io::Error>>,
}
impl Server {
    async fn start(router: Router) -> Result<Self, Error> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stopping.changed().await;
                })
                .await
        });
        Ok(Self { url, stop, task })
    }
    async fn finish(self) -> Result<(), Error> {
        self.stop.send(true).map_err(|_| Error::Internal)?;
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .map_err(|_| Error::Internal)???;
        Ok(())
    }
}
struct Fake {
    snapshot: ProviderSnapshot,
    jobs: BTreeMap<OperationId, SigningJobView>,
    receipts: BTreeMap<OperationId, OperationReceipt>,
    nonces: BTreeMap<AccountId32, u64>,
    submissions: usize,
    lose_reply: bool,
    fail_before_execution: bool,
}
fn info() -> ProviderInfo {
    ProviderInfo {
        provider_instance_id: ProviderInstanceId::from_bytes([1; 16]),
        root_account: AccountId32::from_bytes([9; 32]),
        payload_version: PAYLOAD_VERSION,
    }
}
fn fake() -> Result<Arc<Mutex<Fake>>, ContractError> {
    Ok(Arc::new(Mutex::new(Fake {
        snapshot: ProviderSnapshot {
            info: info(),
            catalog: CatalogView {
                roles: vec![],
                skills: vec![],
                score_policy: ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?,
            },
            workers: vec![],
            projects: vec![],
            balances: vec![],
        },
        jobs: BTreeMap::new(),
        receipts: BTreeMap::new(),
        nonces: BTreeMap::new(),
        submissions: 0,
        lose_reply: false,
        fail_before_execution: false,
    })))
}
fn internal(fake: Arc<Mutex<Fake>>) -> Router {
    Router::new()
        .route(
            "/internal/wallets",
            post(|Json(request): Json<ProvisionWalletRequest>| async move {
                Json(WalletView {
                    wallet_id: WalletId::from_bytes(request.principal_id.into_bytes()),
                    principal_id: Some(request.principal_id),
                    account_id: AccountId32::from_bytes(hash(request.principal_id.as_bytes())),
                    lifecycle: WalletLifecycle::Active,
                })
            }),
        )
        .route(
            "/internal/system-wallet",
            get(|| async {
                Json(WalletView {
                    wallet_id: WalletId::from_bytes([9; 16]),
                    principal_id: None,
                    account_id: info().root_account,
                    lifecycle: WalletLifecycle::Active,
                })
            }),
        )
        .route(
            "/internal/info",
            get(|State(fake): State<Arc<Mutex<Fake>>>| async move {
                Json(fake.lock().await.snapshot.info.clone())
            }),
        )
        .route(
            "/internal/snapshot",
            get(|State(fake): State<Arc<Mutex<Fake>>>| async move {
                Json(fake.lock().await.snapshot.clone())
            }),
        )
        .route(
            "/internal/accounts/{account}/nonce",
            get(
                |State(fake): State<Arc<Mutex<Fake>>>, Path(account): Path<String>| async move {
                    let account = parse(&account)?;
                    Ok::<_, Error>(Json(AccountNonce {
                        account,
                        nonce: *fake.lock().await.nonces.get(&account).unwrap_or(&0),
                    }))
                },
            ),
        )
        .route("/internal/signing-jobs", post(sign))
        .route(
            "/internal/signing-jobs/{operation}",
            get(
                |State(fake): State<Arc<Mutex<Fake>>>, Path(id): Path<String>| async move {
                    Ok::<_, Error>(Json(
                        fake.lock()
                            .await
                            .jobs
                            .get(&parse(&id)?)
                            .cloned()
                            .ok_or(Error::NotFound)?,
                    ))
                },
            ),
        )
        .route("/internal/contracts/call", post(submit))
        .route(
            "/internal/receipts/{operation}",
            get(
                |State(fake): State<Arc<Mutex<Fake>>>, Path(id): Path<String>| async move {
                    Ok::<_, Error>(Json(
                        fake.lock()
                            .await
                            .receipts
                            .get(&parse(&id)?)
                            .cloned()
                            .ok_or(Error::NotFound)?,
                    ))
                },
            ),
        )
        .with_state(fake)
}
async fn sign(
    State(fake): State<Arc<Mutex<Fake>>>,
    Json(request): Json<CreateSigningJobRequest>,
) -> Result<Json<SigningJobView>, Error> {
    let call = UnsignedContractCallV1::decode_signable(&request.signable_payload)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        request.payload_hash,
        payload_hash(&request.signable_payload)
    );
    assert_eq!(request.operation_id, call.operation_id);
    let job = SigningJobView {
        operation_id: request.operation_id,
        wallet_id: request.wallet_id,
        account_id: call.origin,
        status: SigningJobStatus::Signed,
        signature: Some(Sr25519Signature::from_bytes([7; 64])),
        rejection_code: None,
    };
    fake.lock()
        .await
        .jobs
        .insert(request.operation_id, job.clone());
    Ok(Json(job))
}
async fn submit(
    State(fake): State<Arc<Mutex<Fake>>>,
    Json(signed): Json<SignedContractCallV1>,
) -> Result<Response, Error> {
    let mut fake = fake.lock().await;
    let call = signed.call;
    fake.submissions += 1;
    if fake.fail_before_execution {
        return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    if let Some(receipt) = fake.receipts.get(&call.operation_id) {
        return Ok(Json(receipt).into_response());
    }
    assert_eq!(call.nonce, *fake.nonces.get(&call.origin).unwrap_or(&0));
    fake.nonces.insert(call.origin, call.nonce + 1);
    let receipt = OperationReceipt {
        operation_id: call.operation_id,
        provider_instance_id: call.provider_instance_id,
        origin: call.origin,
        nonce: call.nonce,
        outcome: ExecutionOutcome::Success,
        created_entity_id: None,
        first_event_cursor: None,
        last_event_cursor: None,
        finalized_at: UnixSeconds::new(u64::try_from(now()?).map_err(|_| Error::Internal)?),
    };
    fake.receipts.insert(call.operation_id, receipt.clone());
    if fake.lose_reply {
        fake.lose_reply = false;
        return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    Ok(Json(receipt).into_response())
}
async fn app(internal: &Server) -> Result<Arc<App>, Error> {
    let app = Arc::new(
        App::new(Config {
            bind_addr: "127.0.0.1:0".into(),
            database_url: "sqlite::memory:".into(),
            custody_url: internal.url.clone(),
            provider_url: internal.url.clone(),
            service_token: Zeroizing::new("test-service-token-not-a-production-secret".into()),
            allowed_origins: BTreeSet::from([
                "http://localhost:8088".into(),
                "http://localhost:3000".into(),
            ]),
            cookie_secure: false,
            admin_username: "admin".into(),
            admin_password: Zeroizing::new(PASSWORD.into()),
            openapi: "{}".into(),
        })
        .await?,
    );
    auth::bootstrap(&app).await?;
    Ok(app)
}
async fn register(app: &App, name: &str) -> Result<(Session, String), Box<dyn std::error::Error>> {
    let response = auth::register(
        app,
        RegisterRequest {
            username: name.into(),
            password: PASSWORD.into(),
            display_name: name.into(),
        },
    )
    .await?;
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("cookie missing")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("cookie missing")?
        .to_owned();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse()?);
    Ok((auth::authenticate(app, &headers).await?, cookie))
}
fn create() -> ProviderCommand {
    ProviderCommand::CreateProject(CreateProjectRequest {
        title: "A project".into(),
        description: "Description".into(),
    })
}
async fn drive(app: &App, session: &Session, id: OperationId) -> Result<OperationView, Error> {
    for _ in 0..8 {
        sqlx::query("UPDATE operations SET next_attempt_at = 0 WHERE operation_id = ?")
            .bind(id.to_string())
            .execute(&app.db)
            .await?;
        if let Some(job) = operations::claim(app).await? {
            operations::process(app, job).await?;
        }
        let view = operations::read(app, session, id).await?;
        if matches!(
            view.status,
            ProviderOperationStatus::Finalized
                | ProviderOperationStatus::Rejected
                | ProviderOperationStatus::Expired
        ) {
            return Ok(view);
        }
    }
    operations::read(app, session, id).await
}

#[tokio::test]
async fn http_auth_cors_csrf_password_and_typed_commands() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let adapter = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let (user, cookie) = register(&app, "alice").await?;
    let (second_session, second_cookie) = {
        let response = auth::login(
            &app,
            LoginRequest {
                username: "ALICE".into(),
                password: PASSWORD.into(),
            },
        )
        .await?;
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .ok_or("missing cookie")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("missing cookie")?
            .to_owned();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse()?);
        (auth::authenticate(&app, &headers).await?, cookie)
    };
    for origin in ["http://localhost:8088", "http://localhost:3000"] {
        let response = client
            .get(format!("{}/api/auth/session", adapter.url))
            .header(header::COOKIE, &cookie)
            .header(header::ORIGIN, origin)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .ok_or("CORS missing")?,
            origin
        );
        assert_eq!(
            response.json::<SessionView>().await?.account_id,
            user.view.account_id
        );
    }
    let response = client
        .post(format!("{}/api/projects", adapter.url))
        .header(header::COOKIE, &cookie)
        .header(header::ORIGIN, "http://evil.invalid")
        .header("X-CSRF-Token", &user.view.csrf_token)
        .json(&CreateProjectRequest {
            title: "bad origin".into(),
            description: String::new(),
        })
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .post(format!("{}/api/projects", adapter.url))
        .header(header::COOKIE, &cookie)
        .json(&CreateProjectRequest {
            title: "missing csrf".into(),
            description: String::new(),
        })
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .post(format!("{}/api/admin/fund", adapter.url))
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &user.view.csrf_token)
        .json(&FundAccountRequest {
            account: user.view.account_id,
            amount: Money::new(10),
        })
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client.post(format!("{}/api/projects", adapter.url)).header(header::COOKIE, &cookie).header("X-CSRF-Token", &user.view.csrf_token)
        .json(&serde_json::json!({"title":"reject spoofing","description":"", "origin":user.view.account_id})).send().await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = client
        .post(format!("{}/api/projects", adapter.url))
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &user.view.csrf_token)
        .json(&CreateProjectRequest {
            title: "allowed".into(),
            description: String::new(),
        })
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted = response.json::<OperationRef>().await?;
    assert_eq!(accepted.status, ProviderOperationStatus::AwaitingSignature);
    let response = client
        .post(format!("{}/api/auth/password", adapter.url))
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &user.view.csrf_token)
        .json(&ChangePasswordRequest {
            current_password: PASSWORD.into(),
            new_password: "new-independent-password-123".into(),
        })
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        auth::authenticate_hash(&app, second_session.token_hash)
            .await
            .is_err()
    );
    assert_eq!(
        auth::authenticate_hash(&app, user.token_hash)
            .await?
            .view
            .account_id,
        user.view.account_id
    );
    assert!(
        auth::login(
            &app,
            LoginRequest {
                username: "alice".into(),
                password: PASSWORD.into()
            }
        )
        .await
        .is_err()
    );
    let response = client
        .get(format!("{}/api/auth/session", adapter.url))
        .header(header::COOKIE, second_cookie)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    auth::logout(&app, &user).await?;
    assert!(
        auth::authenticate_hash(&app, user.token_hash)
            .await
            .is_err()
    );
    adapter.finish().await?;
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn queue_is_owned_ordered_leased_and_recovers_lost_reply() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "alice").await?;
    let (bob, _) = register(&app, "bobby").await?;
    let key = OperationId::from_bytes([4; 16]);
    let first = operations::enqueue(&app, &alice, create(), Some(key)).await?;
    assert_eq!(
        first,
        operations::enqueue(&app, &alice, create(), Some(key)).await?
    );
    assert!(
        operations::enqueue(&app, &bob, create(), Some(key))
            .await
            .is_err()
    );
    assert!(operations::read(&app, &bob, key).await.is_err());
    let mut changed = create();
    if let ProviderCommand::CreateProject(request) = &mut changed {
        request.title = "Different action".into();
    }
    assert!(
        operations::enqueue(&app, &alice, changed, Some(key))
            .await
            .is_err()
    );
    let second = operations::enqueue(&app, &alice, create(), None).await?;
    let third = operations::enqueue(&app, &bob, create(), None).await?;
    let claim_a = operations::claim(&app).await?.ok_or("missing job")?;
    let claim_b = operations::claim(&app)
        .await?
        .ok_or("unrelated wallet blocked")?;
    assert!(operations::claim(&app).await?.is_none());
    let rows = sqlx::query("SELECT operation_id FROM operations WHERE lease_token IS NOT NULL ORDER BY creation_sequence").fetch_all(&app.db).await?;
    assert_eq!(
        rows[0].try_get::<String, _>("operation_id")?,
        key.to_string()
    );
    assert_eq!(
        rows[1].try_get::<String, _>("operation_id")?,
        third.operation_id.to_string()
    );
    // Expired lease holders cannot overwrite a replacement claim's work.
    sqlx::query("UPDATE operations SET lease_until = 0 WHERE operation_id = ?")
        .bind(key.to_string())
        .execute(&app.db)
        .await?;
    let reclaimed = operations::claim(&app)
        .await?
        .ok_or("lease not reclaimed")?;
    operations::process(&app, claim_a).await?;
    let row = sqlx::query("SELECT signable_payload FROM operations WHERE operation_id = ?")
        .bind(key.to_string())
        .fetch_one(&app.db)
        .await?;
    assert!(
        row.try_get::<Option<Vec<u8>>, _>("signable_payload")?
            .is_none()
    );
    operations::process(&app, reclaimed).await?;
    operations::process(&app, claim_b).await?;
    fake.lock().await.lose_reply = true;
    let first = drive(&app, &alice, key).await?;
    assert_eq!(first.status, ProviderOperationStatus::Finalized);
    assert_eq!(first.receipt.ok_or("missing receipt")?.nonce, 0);
    let second = drive(&app, &alice, second.operation_id).await?;
    assert_eq!(second.status, ProviderOperationStatus::Finalized);
    assert_eq!(second.receipt.ok_or("missing second receipt")?.nonce, 1);
    assert_eq!(fake.lock().await.submissions, 2);
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn exhausted_submissions_and_provider_reset_remain_unknown() -> TestResult {
    let fake = fake()?;
    fake.lock().await.fail_before_execution = true;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "alice").await?;
    let first = operations::enqueue(&app, &alice, create(), None).await?;
    let queued = operations::enqueue(&app, &alice, create(), None).await?;
    let unresolved = drive(&app, &alice, first.operation_id).await?;
    assert_eq!(unresolved.status, ProviderOperationStatus::OutcomeUnknown);
    assert!(unresolved.receipt.is_none());
    assert_eq!(fake.lock().await.submissions, 5);
    let before = sqlx::query("SELECT signable_payload, signed_json, provider_instance_id FROM operations WHERE operation_id = ?")
        .bind(first.operation_id.to_string()).fetch_one(&app.db).await?;
    let original_bytes: Vec<u8> = before.try_get("signable_payload")?;
    let original_signed: String = before.try_get("signed_json")?;
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([6; 16]);
    let unresolved = drive(&app, &alice, first.operation_id).await?;
    assert_eq!(unresolved.status, ProviderOperationStatus::OutcomeUnknown);
    assert_eq!(
        unresolved.error_code.as_deref(),
        Some("provider_instance_changed")
    );
    assert_eq!(fake.lock().await.submissions, 5);
    assert_eq!(
        operations::read(&app, &alice, queued.operation_id)
            .await?
            .status,
        ProviderOperationStatus::AwaitingSignature
    );
    let after = sqlx::query("SELECT signable_payload, signed_json, provider_instance_id FROM operations WHERE operation_id = ?")
        .bind(first.operation_id.to_string()).fetch_one(&app.db).await?;
    assert_eq!(
        after.try_get::<Vec<u8>, _>("signable_payload")?,
        original_bytes
    );
    assert_eq!(after.try_get::<String, _>("signed_json")?, original_signed);
    assert_eq!(
        after.try_get::<String, _>("provider_instance_id")?,
        info().provider_instance_id.to_string()
    );
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn only_client_and_assigned_coordinator_can_cancel_or_dispute() -> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (client, _) = register(&app, "client").await?;
    let (coordinator, _) = register(&app, "coordinator").await?;
    let (outsider, _) = register(&app, "outsider").await?;
    let project_id = EntityId::from_bytes([8; 16]);
    fake.lock().await.snapshot.projects.push(ProjectView {
        project_id,
        client: client.view.account_id,
        coordinator: coordinator.view.account_id,
        title: "Permission boundary".into(),
        description: String::new(),
        planning: PlanningView {
            revision: 0,
            status: PlanningStatus::AwaitingQuote,
            quote: None,
            escrow: Money::ZERO,
            frozen: false,
        },
        proposals: vec![],
        execution_escrow: Money::ZERO,
        cancelled: false,
    });
    let request = ReasonRequest {
        reason: "Review required".into(),
    };
    for command in [
        ProviderCommand::CancelProject {
            project_id,
            request: request.clone(),
        },
        ProviderCommand::DisputePlanning {
            project_id,
            request: request.clone(),
        },
        ProviderCommand::DisputeMilestone {
            project_id,
            milestone_id: EntityId::from_bytes([7; 16]),
            request,
        },
    ] {
        operations::authorize(&app, &client, &command).await?;
        operations::authorize(&app, &coordinator, &command).await?;
        assert!(matches!(
            operations::authorize(&app, &outsider, &command).await,
            Err(Error::Forbidden)
        ));
    }
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn notifications_are_atomic_private_resumable_and_explicitly_read() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, cookie) = register(&app, "alice").await?;
    let (bob, _) = register(&app, "bobby").await?;
    let event = |cursor, recipient| DomainEvent {
        provider_instance_id: info().provider_instance_id,
        cursor,
        operation_id: OperationId::from_bytes([4; 16]),
        kind: DomainEventKind::ProjectCreated,
        project_id: Some(EntityId::from_bytes([8; 16])),
        entity_id: None,
        recipients: vec![recipient],
        occurred_at: UnixSeconds::new(1000),
    };
    let provider_page = ProviderEvents {
        provider_instance_id: info().provider_instance_id,
        events: vec![
            event(1, alice.view.account_id),
            event(2, bob.view.account_id),
            event(3, alice.view.account_id),
        ],
        next_cursor: 3,
    };
    notifications::persist(&app, 0, provider_page.clone()).await?;
    notifications::persist(&app, 0, provider_page).await?;
    let first = notifications::page(&app, alice.view.account_id, 0).await?;
    assert_eq!(first.notifications.len(), 2);
    assert!(first.notifications.iter().all(|n| n.read_at.is_none()));
    let cursor = first.notifications[0].notification_id;
    assert_eq!(
        notifications::page(&app, alice.view.account_id, cursor)
            .await?
            .notifications
            .len(),
        1
    );
    assert!(
        notifications::mark_read(&app, bob.view.account_id, cursor)
            .await
            .is_err()
    );
    let adapter = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let mut response = client
        .get(format!("{}/api/events?after=0", adapter.url))
        .header(header::COOKIE, &cookie)
        .header("Last-Event-ID", cursor.to_string())
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = tokio::time::timeout(Duration::from_secs(5), response.chunk())
        .await??
        .ok_or("missing event")?;
    let body = std::str::from_utf8(&bytes)?;
    assert!(body.contains("event: notification"));
    assert!(body.contains(&format!("id: {}", first.next_cursor)));
    assert!(!body.contains(&bob.view.account_id.to_string()));
    drop(response);
    assert!(
        notifications::page(&app, alice.view.account_id, 0)
            .await?
            .notifications
            .iter()
            .all(|n| n.read_at.is_none())
    );
    let marked = notifications::mark_read(&app, alice.view.account_id, cursor).await?;
    assert!(marked.read_at.is_some());
    assert_eq!(
        marked,
        notifications::mark_read(&app, alice.view.account_id, cursor).await?
    );
    let bad = ProviderEvents {
        provider_instance_id: ProviderInstanceId::from_bytes([3; 16]),
        events: vec![event(4, alice.view.account_id)],
        next_cursor: 4,
    };
    assert!(notifications::persist(&app, 0, bad).await.is_err());
    let reset = ProviderEvents {
        provider_instance_id: ProviderInstanceId::from_bytes([3; 16]),
        events: vec![],
        next_cursor: 0,
    };
    notifications::persist(&app, 0, reset).await?;
    assert_eq!(
        notifications::page(&app, alice.view.account_id, 0)
            .await?
            .notifications
            .len(),
        2
    );
    adapter.finish().await?;
    internal.finish().await?;
    Ok(())
}
