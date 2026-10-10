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

#[path = "project_briefs/http_tests.rs"]
mod project_briefs;

#[path = "project_participants_tests.rs"]
mod project_participants;

#[path = "proposal_reviews/http_tests.rs"]
mod proposal_reviews;
#[path = "submission_presentations_tests.rs"]
mod submission_presentations;

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
                skill_roles: vec![],
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
    let base = std::env::var("TEST_ADAPTER_DATABASE_URL").map_err(|_| Error::Config)?;
    let schema = format!(
        "adapter_test_{:032x}",
        u128::from_le_bytes(crate::state::random()?)
    );
    let admin = sqlx::PgPool::connect(&base).await?;
    // `schema` contains only a fixed prefix and locally generated lowercase hex digits.
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    admin.close().await;
    let separator = if base.contains('?') { '&' } else { '?' };
    let database_url = format!("{base}{separator}options=-csearch_path%3D{schema}");
    let app = Arc::new(App::new(test_config(database_url, internal)).await?);
    auth::bootstrap(&app).await?;
    Ok(app)
}
fn test_config(database_url: String, internal: &Server) -> Config {
    Config {
        bind_addr: "127.0.0.1:0".into(),
        database_url,
        custody_url: internal.url.clone(),
        provider_url: internal.url.clone(),
        service_token: Zeroizing::new("test-service-token-not-a-production-secret".into()),
        allowed_origins: BTreeSet::from([
            "http://localhost:8088".into(),
            "http://localhost:3000".into(),
        ]),
        cookie_secure: false,
        enable_mock_funding: true,
        dispute_channel_allow_participants: false,
        admin_username: "admin".into(),
        admin_password: Zeroizing::new(PASSWORD.into()),
        openapi: "{}".into(),
    }
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
        sqlx::query("UPDATE operations SET next_attempt_at = 0 WHERE operation_id = $1")
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
async fn profile_images_are_bounded_public_and_owner_written() -> TestResult {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0bIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\xa5\xf6E@\x00\x00\x00\x00IEND\xaeB`\x82";
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let adapter = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let (owner, cookie) = register(&app, "image-owner").await?;
    let (outsider, outsider_cookie) = register(&app, "image-outsider").await?;
    let profile_url = format!("{}/api/profiles/me", adapter.url);
    let upload_url = format!("{}/api/profiles/me/client/image", adapter.url);
    let public_url = format!(
        "{}/api/profiles/{}/client/image",
        adapter.url, owner.view.principal_id
    );

    let missing = client.get(&public_url).send().await?;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let profile = serde_json::json!({
        "section": "client", "profile": {
            "name": "Image owner", "company": null, "department": null,
            "website": null, "description": null, "location": null,
            "languages": []
        }
    });
    let saved = client
        .put(&profile_url)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &owner.view.csrf_token)
        .json(&profile)
        .send()
        .await?;
    assert_eq!(saved.status(), StatusCode::OK);
    let no_csrf = client
        .put(&upload_url)
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await?;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);
    let uploaded = client
        .put(&upload_url)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &owner.view.csrf_token)
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await?;
    assert_eq!(uploaded.status(), StatusCode::NO_CONTENT);
    let public = client.get(&public_url).send().await?;
    assert_eq!(public.status(), StatusCode::OK);
    assert_eq!(public.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(public.headers()["x-content-type-options"], "nosniff");
    assert_eq!(public.bytes().await?.as_ref(), PNG);

    let worker_profile = serde_json::json!({
        "section": "worker", "profile": {
            "name": "Image worker", "githubUsername": null, "portfolioUrl": null,
            "biography": null, "background": null, "proficiency": null,
            "location": null, "languages": []
        }
    });
    assert_eq!(
        client
            .put(&profile_url)
            .header(header::COOKIE, &cookie)
            .header("x-csrf-token", &owner.view.csrf_token)
            .json(&worker_profile)
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    let worker_upload = format!("{}/api/profiles/me/worker/image", adapter.url);
    let worker_public = format!(
        "{}/api/profiles/{}/worker/image",
        adapter.url, owner.view.principal_id
    );
    let mut larger_image = PNG.to_vec();
    larger_image.resize(MAX_SIGNABLE_BYTES + 1, 0);
    assert_eq!(
        client
            .put(worker_upload)
            .header(header::COOKIE, &cookie)
            .header("x-csrf-token", &owner.view.csrf_token)
            .header(header::CONTENT_TYPE, "image/png")
            .body(larger_image.clone())
            .send()
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .get(worker_public)
            .send()
            .await?
            .bytes()
            .await?
            .as_ref(),
        larger_image.as_slice()
    );

    let unauthorized = client
        .put(&upload_url)
        .header(header::COOKIE, &outsider_cookie)
        .header("x-csrf-token", &outsider.view.csrf_token)
        .header(header::CONTENT_TYPE, "image/png")
        .body(PNG.to_vec())
        .send()
        .await?;
    assert_eq!(unauthorized.status(), StatusCode::NOT_FOUND);
    let invalid = client
        .put(&upload_url)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &owner.view.csrf_token)
        .header(header::CONTENT_TYPE, "image/png")
        .body(b"<script>bad</script>".to_vec())
        .send()
        .await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let unsupported = client
        .put(&upload_url)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &owner.view.csrf_token)
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .body(b"<svg/>".to_vec())
        .send()
        .await?;
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    let oversized = client
        .put(&upload_url)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &owner.view.csrf_token)
        .header(header::CONTENT_TYPE, "image/png")
        .body(vec![0_u8; crate::profiles::MAX_IMAGE_BYTES + 1])
        .send()
        .await?;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        client
            .get(&public_url)
            .send()
            .await?
            .bytes()
            .await?
            .as_ref(),
        PNG
    );

    adapter.finish().await?;
    internal.finish().await?;
    Ok(())
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
async fn passkey_username_login_keeps_the_existing_custodial_account() -> TestResult {
    use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
    use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

    const ORIGIN: &str = "http://localhost:8088";
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let adapter = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let (alice, cookie) = register(&app, "alice").await?;
    register(&app, "bob").await?;
    let origin = Url::parse(ORIGIN)?;

    for name in ["bob", "unknown", "bad username!"] {
        let response = client
            .post(format!("{}/api/auth/passkeys/login/options", adapter.url))
            .header(header::ORIGIN, ORIGIN)
            .json(&serde_json::json!({"username": name}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        assert_eq!(
            response.json::<ApiError>().await?.code,
            "invalid_credentials"
        );
    }

    let response = client
        .post(format!(
            "{}/api/auth/passkeys/register/options",
            adapter.url
        ))
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &alice.view.csrf_token)
        .json(&serde_json::json!({"currentPassword": PASSWORD}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let registration: serde_json::Value = response.json().await?;
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let options: CreationChallengeResponse =
        serde_json::from_value(registration["options"].clone())?;
    let credential = authenticator.do_registration(origin.clone(), options)?;
    let response = client
        .post(format!("{}/api/auth/passkeys/register/verify", adapter.url))
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &alice.view.csrf_token)
        .json(&serde_json::json!({
            "ceremonyId": registration["ceremonyId"],
            "credential": credential,
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = client
        .post(format!("{}/api/auth/passkeys/login/options", adapter.url))
        .header(header::ORIGIN, ORIGIN)
        .json(&serde_json::json!({"username": "ALICE"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let login: serde_json::Value = response.json().await?;
    let options: RequestChallengeResponse = serde_json::from_value(login["options"].clone())?;
    let assertion = authenticator.do_authentication(origin, options)?;
    let proof = serde_json::json!({
        "ceremonyId": login["ceremonyId"],
        "credential": assertion,
    });
    let response = client
        .post(format!("{}/api/auth/passkeys/login/verify", adapter.url))
        .json(&proof)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let passkey_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("passkey session cookie missing")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("passkey session cookie missing")?
        .to_owned();
    let view: SessionView = response.json().await?;
    assert_eq!(view.principal_id, alice.view.principal_id);
    assert_eq!(view.account_id, alice.view.account_id);
    let response = client
        .get(format!("{}/api/auth/session", adapter.url))
        .header(header::COOKIE, passkey_cookie)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<SessionView>().await?.account_id,
        alice.view.account_id
    );
    let response = client
        .post(format!("{}/api/auth/passkeys/login/verify", adapter.url))
        .json(&proof)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

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
    sqlx::query("UPDATE operations SET lease_until = 0 WHERE operation_id = $1")
        .bind(key.to_string())
        .execute(&app.db)
        .await?;
    let reclaimed = operations::claim(&app)
        .await?
        .ok_or("lease not reclaimed")?;
    operations::process(&app, claim_a).await?;
    let row = sqlx::query("SELECT signable_payload FROM operations WHERE operation_id = $1")
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
    let before = sqlx::query("SELECT signable_payload, signed_json, provider_instance_id FROM operations WHERE operation_id = $1")
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
    let after = sqlx::query("SELECT signable_payload, signed_json, provider_instance_id FROM operations WHERE operation_id = $1")
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
        completed: false,
        evaluations: Vec::new(),
        project_id,
        client: client.view.account_id,
        coordinator: coordinator.view.account_id,
        created_at: Some(UnixSeconds::new(1_788_912_000)),
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
        active_dispute_id: None,
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
        ProviderCommand::OpenDispute(OpenDisputeRequest {
            project_id,
            milestone_id: EntityId::from_bytes([7; 16]),
            rejected_submission_id: EntityId::from_bytes([6; 16]),
            evidence: EvidenceReference::new("https://example.test/reason".to_owned())?,
        }),
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
        origin: None,
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

#[tokio::test]
async fn postgres_restart_preserves_queue_and_unread_notifications() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let (alice, _) = register(&app, "alice").await?;
    let operation = operations::enqueue(&app, &alice, create(), None).await?;
    let event = DomainEvent {
        provider_instance_id: info().provider_instance_id,
        cursor: 1,
        operation_id: operation.operation_id,
        kind: DomainEventKind::ProjectCreated,
        project_id: None,
        entity_id: None,
        recipients: vec![alice.view.account_id],
        occurred_at: UnixSeconds::new(1000),
        origin: None,
    };
    notifications::persist(
        &app,
        0,
        ProviderEvents {
            provider_instance_id: info().provider_instance_id,
            events: vec![event],
            next_cursor: 1,
        },
    )
    .await?;
    let (first, second) = tokio::join!(operations::claim(&app), operations::claim(&app));
    assert_eq!(
        usize::from(first?.is_some()) + usize::from(second?.is_some()),
        1
    );
    sqlx::query("UPDATE operations SET lease_until = 0 WHERE operation_id = $1")
        .bind(operation.operation_id.to_string())
        .execute(&app.db)
        .await?;
    let url = app.config.database_url.clone();
    drop(app);
    let reopened = App::new(test_config(url, &internal)).await?;
    let job = operations::claim(&reopened)
        .await?
        .ok_or("lease not recovered")?;
    operations::process(&reopened, job).await?;
    assert_eq!(
        drive(&reopened, &alice, operation.operation_id)
            .await?
            .status,
        ProviderOperationStatus::Finalized
    );
    let unread = notifications::page(&reopened, alice.view.account_id, 0).await?;
    assert_eq!(unread.notifications.len(), 1);
    assert!(unread.notifications[0].read_at.is_none());
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn signup_catalog_is_public_and_contact_email_is_private() -> TestResult {
    let internal = Server::start(internal(fake()?)).await?;
    let app = app(&internal).await?;
    let adapter = Server::start(http::router(app.clone())).await?;
    let client = reqwest::Client::new();
    let catalog_url = format!("{}/api/catalog", adapter.url);
    let response = client.get(&catalog_url).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    response.json::<CatalogView>().await?;
    assert_eq!(
        client
            .get(&catalog_url)
            .header(header::ORIGIN, "http://evil.invalid")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    for path in ["/api/workers", "/api/profiles/me"] {
        assert_eq!(
            client
                .get(format!("{}{path}", adapter.url))
                .send()
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        client
            .post(format!("{}/api/catalog/skill-requests", adapter.url))
            .json(&serde_json::json!({}))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .put(format!("{}/api/admin/catalog", adapter.url))
            .json(&serde_json::json!({}))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (owner, cookie) = register(&app, "contact-owner").await?;
    let (_, other_cookie) = register(&app, "contact-other").await?;
    let profile_url = format!("{}/api/profiles/me", adapter.url);
    let mut body = serde_json::json!({"section":"worker", "profile":{"name":"Worker", "contactEmail":"worker@example.test",
        "githubUsername":null, "portfolioUrl":null, "biography":null, "background":null,
        "proficiency":null, "location":null, "languages":[]}});
    let response = client
        .put(&profile_url)
        .header(header::COOKIE, &cookie)
        .header("X-CSRF-Token", &owner.view.csrf_token)
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await?["worker"]["contactEmail"],
        "worker@example.test"
    );
    let private = client
        .get(&profile_url)
        .header(header::COOKIE, &cookie)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    assert_eq!(private["worker"]["contactEmail"], "worker@example.test");
    let public = client
        .get(format!(
            "{}/api/profiles/{}",
            adapter.url, owner.view.principal_id
        ))
        .header(header::COOKIE, &other_cookie)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    assert!(public["worker"].get("contactEmail").is_none());
    let other = client
        .get(&profile_url)
        .header(header::COOKIE, &other_cookie)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    assert!(other["worker"].is_null());
    body["profile"]["contactEmail"] = serde_json::json!("bad email");
    assert_eq!(
        client
            .put(&profile_url)
            .header(header::COOKIE, &cookie)
            .header("X-CSRF-Token", &owner.view.csrf_token)
            .json(&body)
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    for remove in [false, true] {
        body["profile"]["contactEmail"] = serde_json::Value::Null;
        if remove {
            body["profile"]
                .as_object_mut()
                .ok_or("profile missing")?
                .remove("contactEmail");
        }
        let response = client
            .put(&profile_url)
            .header(header::COOKIE, &cookie)
            .header("X-CSRF-Token", &owner.view.csrf_token)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.json::<serde_json::Value>().await?["worker"]["contactEmail"].is_null());
    }
    adapter.finish().await?;
    internal.finish().await?;
    Ok(())
}

#[tokio::test]
async fn notifications_enrich_verified_author_and_visible_project_without_private_contacts()
-> TestResult {
    let fake = fake()?;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let (owner, _) = register(&app, "notification_client").await?;
    let (coordinator, _) = register(&app, "notification_coordinator").await?;
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let mut project: ProjectView =
        serde_json::from_value(doc["components"]["schemas"]["ProjectView"]["examples"][0].clone())?;
    project.client = owner.view.account_id;
    project.coordinator = coordinator.view.account_id;
    project.title = "Real project title".into();
    fake.lock().await.snapshot.projects.push(project.clone());
    sqlx::query("INSERT INTO worker_profiles(principal_id,name,contact_email,languages,image_data,image_mime_type,updated_at) VALUES($1,'Real coordinator','private@example.test',ARRAY[]::text[],$2,'image/png',1)")
        .bind(coordinator.view.principal_id.to_string()).bind(vec![1_u8,2,3]).execute(&app.db).await?;
    let event = DomainEvent {
        provider_instance_id: info().provider_instance_id,
        cursor: 1,
        operation_id: OperationId::from_bytes([8; 16]),
        kind: DomainEventKind::ProposalSubmitted,
        project_id: Some(project.project_id),
        entity_id: None,
        recipients: vec![owner.view.account_id],
        occurred_at: UnixSeconds::new(1000),
        origin: Some(coordinator.view.account_id),
    };
    notifications::persist(
        &app,
        0,
        ProviderEvents {
            provider_instance_id: info().provider_instance_id,
            events: vec![event],
            next_cursor: 1,
        },
    )
    .await?;
    let page = notifications::page(&app, owner.view.account_id, 0).await?;
    let notification = &page.notifications[0];
    assert_eq!(
        notification.project_title.as_deref(),
        Some("Real project title")
    );
    let actor = notification.actor.as_ref().ok_or("missing actor")?;
    assert_eq!(actor.account_id, coordinator.view.account_id);
    assert_eq!(actor.display_name, "Real coordinator");
    assert_eq!(
        actor.image_url,
        Some(format!(
            "/api/profiles/{}/worker/image",
            coordinator.view.principal_id
        ))
    );
    assert!(!serde_json::to_string(notification)?.contains("private@example.test"));
    let read =
        notifications::mark_read(&app, owner.view.account_id, notification.notification_id).await?;
    assert_eq!(read.actor, notification.actor);
    assert_eq!(read.project_title, notification.project_title);
    assert!(
        notifications::page(&app, coordinator.view.account_id, 0)
            .await?
            .notifications
            .is_empty()
    );
    fake.lock().await.snapshot.projects.clear();
    assert!(
        notifications::page(&app, owner.view.account_id, 0)
            .await?
            .notifications[0]
            .project_title
            .is_none()
    );
    internal.finish().await?;
    Ok(())
}

#[path = "submission_comments_tests.rs"]
mod submission_comments;

#[path = "disputes/conversations_tests.rs"]
mod dispute_conversations;
