use crate::{
    auth::{self, Session},
    notifications, operations,
    state::{App, Error, parse, public_error},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Extension, MatchedPath, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use generated_contracts::{
    BalanceView, CalendarSummaryView, CatalogKind, CatalogView, DeleteCatalogEntryRequest,
    EntityId, MAX_SIGNABLE_BYTES, Minutes, Money, NotificationView, NotificationsPage,
    OperationRef, OperationView, ProjectView, ProviderCommand, ProviderInfo, RevisionRequest,
    SessionView, TaskStorageView, TaskView, WalletView, Week, WeeklyCommitment, WorkerSummaryView,
};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[allow(clippy::too_many_lines)] // Keep declarative routes together.
pub(crate) fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/ready", get(ready))
        .route("/api/openapi.json", get(openapi))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/session", get(session))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(password))
        .route(
            "/api/auth/passkeys/register/options",
            post(crate::auth::passkeys::register_options),
        )
        .route(
            "/api/auth/passkeys/register/verify",
            post(crate::auth::passkeys::register_verify),
        )
        .route("/api/auth/passkeys", get(crate::auth::passkeys::list))
        .route(
            "/api/auth/passkeys/{credentialId}/remove",
            post(crate::auth::passkeys::remove),
        )
        .route(
            "/api/auth/passkeys/login/options",
            post(crate::auth::passkeys::login_options),
        )
        .route(
            "/api/auth/passkeys/login/verify",
            post(crate::auth::passkeys::login_verify),
        )
        .route(
            "/api/profiles/me",
            get(crate::profiles::get_me).put(crate::profiles::put_me),
        )
        .route(
            "/api/profiles/{principalId}",
            get(crate::profiles::get_public),
        )
        .route(
            "/api/profiles/me/{section}/image",
            put(crate::profiles::put_image)
                .layer(DefaultBodyLimit::max(crate::profiles::MAX_IMAGE_BYTES)),
        )
        .route(
            "/api/profiles/{principalId}/{section}/image",
            get(crate::profiles::get_image),
        )
        .route(
            "/api/completion-submissions/{submissionId}/comments",
            get(crate::submission_comments::get).post(crate::submission_comments::post),
        )
        .route(
            "/api/completion-submissions/{submissionId}/presentation",
            get(crate::submission_presentations::get).put(crate::submission_presentations::put),
        )
        .route("/api/catalog", get(catalog))
        .route("/api/catalog/skill-requests", post(command))
        .route("/api/catalog/skill-requests/me", get(crate::catalog::mine))
        .route(
            "/api/admin/catalog/skill-requests",
            get(crate::catalog::all),
        )
        .route(
            "/api/admin/catalog/skill-requests/{requestId}/decision",
            post(command),
        )
        .route("/api/bramp/deposits", post(command))
        .route(
            "/api/bramp/deposits/{depositId}",
            get(crate::bramp::deposit),
        )
        .route(
            "/api/admin/bramp/deposits/{depositId}/confirm",
            post(command),
        )
        .route("/api/bramp/withdrawals", post(command))
        .route(
            "/api/bramp/withdrawals/{withdrawalId}",
            get(crate::bramp::withdrawal),
        )
        .route(
            "/api/bramp/withdrawals/{withdrawalId}/cancel",
            post(command),
        )
        .route("/api/workers", get(workers).post(command))
        .route("/api/workers/me/qualifications", put(command))
        .route("/api/workers/me/calendar", put(command))
        .route("/api/workers/me/mode", put(command))
        .merge(marketplace_routes())
        .route("/api/admin/coordinators", post(command))
        .route("/api/admin/catalog", put(command))
        .route(
            "/api/admin/catalog/{kind}/{id}",
            axum::routing::delete(command),
        )
        .route("/api/admin/score-policy", put(command))
        .route("/api/admin/fund", post(command))
        .route("/api/balance", get(balance))
        .route("/api/operations/{operationId}", get(operation))
        .route("/api/notifications", get(notification_page))
        .route("/api/notifications/{notificationId}/read", post(mark_read))
        .route("/api/events", get(events))
        .fallback(|| async { Error::NotFound })
        .layer(DefaultBodyLimit::max(MAX_SIGNABLE_BYTES))
        .layer(middleware::from_fn_with_state(app.clone(), boundary))
        .with_state(app)
}

#[allow(
    clippy::too_many_lines,
    reason = "Explicit marketplace route registration keeps the public surface reviewable."
)]
fn marketplace_routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/projects", get(projects).post(command))
        .route(
            "/api/completion-submissions/{submissionId}/rejection",
            post(command),
        )
        .route("/api/disputes", post(command))
        .route(
            "/api/disputes/{disputeId}",
            get(crate::disputes::public_case),
        )
        .route(
            "/api/disputes/{disputeId}/presentation",
            get(crate::disputes::conversations::presentation),
        )
        .route("/api/disputes/{disputeId}/response", post(command))
        .route(
            "/api/disputes/{disputeId}/arguments",
            get(crate::disputes::conversations::arguments)
                .post(crate::disputes::conversations::post_argument),
        )
        .route(
            "/api/disputes/{disputeId}/history",
            get(crate::disputes::conversations::history),
        )
        .route(
            "/api/disputes/{disputeId}/messages",
            get(crate::disputes::conversations::messages)
                .post(crate::disputes::conversations::post_message),
        )
        .route("/api/projects/{projectId}", get(project))
        .route("/api/projects/{projectId}/evaluations", post(command))
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/presentation",
            get(crate::proposal_reviews::get_presentation)
                .put(crate::proposal_reviews::put_presentation),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/comments",
            get(crate::proposal_reviews::get_comments).post(crate::proposal_reviews::post_comment),
        )
        .route(
            "/api/projects/{projectId}/participants",
            get(crate::project_participants::get),
        )
        .route(
            "/api/projects/{projectId}/brief",
            get(crate::project_briefs::get).put(crate::project_briefs::put),
        )
        .route("/api/projects/{projectId}/planning/quote", post(command))
        .route("/api/projects/{projectId}/planning/accept", post(command))
        .route(
            "/api/projects/{projectId}/planning/accept-delivery",
            post(command),
        )
        .route("/api/projects/{projectId}/planning/dispute", post(command))
        .route("/api/projects/{projectId}/proposals", post(command))
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}",
            put(command).delete(command),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/delivery",
            put(command),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/withdraw",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/submit",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/approve",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/proposals/{proposalId}/changes",
            post(command),
        )
        .route("/api/projects/{projectId}/cancel", post(command))
        .route(
            "/api/projects/{projectId}/milestones/{milestoneId}/request-completion",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/milestones/{milestoneId}/accept-completion",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/milestones/{milestoneId}/completion-submissions",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/task-storages/{storageId}/tasks",
            post(command),
        )
        .route(
            "/api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}",
            put(command).delete(command),
        )
        .route(
            "/api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}/progress",
            axum::routing::patch(command),
        )
        .route("/api/task-storages/{storageId}", get(task_storage))
        .route("/api/task-storages/{storageId}/tasks", post(command))
        .route(
            "/api/task-storages/{storageId}/tasks/{taskId}",
            get(task).put(command).patch(command).delete(command),
        )
}

fn is_public_case(request: &Request) -> bool {
    matches!(*request.method(), Method::GET | Method::HEAD)
        && request
            .extensions()
            .get::<MatchedPath>()
            .is_some_and(|path| {
                matches!(
                    path.as_str(),
                    "/api/disputes/{disputeId}"
                        | "/api/disputes/{disputeId}/presentation"
                        | "/api/disputes/{disputeId}/arguments"
                        | "/api/disputes/{disputeId}/history"
                        | "/api/profiles/{principalId}"
                        | "/api/profiles/{principalId}/{section}/image"
                )
            })
}

async fn boundary(State(app): State<Arc<App>>, mut request: Request, next: Next) -> Response {
    let Ok(_permit) = app.request_slots.try_acquire() else {
        return Error::Capacity.into_response();
    };
    let origin = request.headers().get(header::ORIGIN).cloned();
    if origin.as_ref().is_some_and(|v| {
        v.to_str()
            .ok()
            .is_none_or(|s| !app.config.allowed_origins.contains(s))
    }) {
        return public_error(StatusCode::FORBIDDEN, "origin_not_allowed");
    }
    let path = request.uri().path();
    let public = is_public_case(&request)
        || (request.method() == Method::GET && path == "/api/catalog")
        || matches!(
            path,
            "/health"
                | "/ready"
                | "/api/openapi.json"
                | "/api/auth/login"
                | "/api/auth/register"
                | "/api/auth/passkeys/login/options"
                | "/api/auth/passkeys/login/verify"
        );
    let mutation = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if mutation && origin.is_none() && request.headers().get("sec-fetch-site").is_some() {
        return public_error(StatusCode::FORBIDDEN, "origin_not_allowed");
    }
    let mut response = if request.method() == Method::OPTIONS {
        if origin.is_none() {
            return Error::Invalid.into_response();
        }
        let allowed = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|m| matches!(m, "GET" | "POST" | "PUT" | "PATCH" | "DELETE"));
        let headers_allowed = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .is_none_or(|v| {
                v.to_str().is_ok_and(|s| {
                    s.split(',').all(|h| {
                        matches!(
                            h.trim().to_ascii_lowercase().as_str(),
                            "content-type" | "x-csrf-token" | "idempotency-key" | "last-event-id"
                        )
                    })
                })
            });
        if !allowed || !headers_allowed {
            return Error::Forbidden.into_response();
        }
        StatusCode::NO_CONTENT.into_response()
    } else {
        if !public && path.starts_with("/api/") {
            match auth::authenticate(&app, request.headers()).await {
                Ok(session) => {
                    if mutation && !auth::valid_csrf(&session, request.headers()) {
                        return with_cors(
                            public_error(StatusCode::FORBIDDEN, "csrf_invalid"),
                            origin,
                        );
                    }
                    request.extensions_mut().insert(session);
                }
                Err(error) => return with_cors(error.into_response(), origin),
            }
        }
        next.run(request).await
    };
    // Axum extractor rejections must not return raw input or a second error format.
    if response.status().is_client_error()
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        response = public_error(
            response.status(),
            if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
                "request_too_large"
            } else {
                "invalid_request"
            },
        );
    }
    with_cors(response, origin)
}
fn with_cors(mut response: Response, origin: Option<HeaderValue>) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    if let Some(origin) = origin {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Content-Type, X-CSRF-Token, Idempotency-Key, Last-Event-ID"),
        );
    }
    response
}
async fn ready(State(app): State<Arc<App>>) -> Result<StatusCode, Error> {
    sqlx::query("SELECT 1").execute(&app.db).await?;
    let _: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
    let _: WalletView = app
        .get(&app.config.custody_url, "/internal/system-wallet")
        .await?;
    Ok(StatusCode::OK)
}
async fn openapi(State(app): State<Arc<App>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        app.config.openapi.clone(),
    )
}
fn json<T: DeserializeOwned>(headers: &HeaderMap, bytes: &[u8]) -> Result<T, Error> {
    if !headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|mime| mime.trim() == "application/json")
        })
    {
        return Err(Error::Invalid);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Invalid)
}
async fn register(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    auth::register(&app, json(&headers, &body)?).await
}
async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    auth::login(&app, json(&headers, &body)?).await
}
async fn session(Extension(session): Extension<Session>) -> Json<SessionView> {
    Json(session.view)
}
async fn logout(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Response, Error> {
    auth::logout(&app, &session).await
}
async fn password(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, Error> {
    auth::change_password(&app, &session, json(&headers, &body)?).await
}
async fn catalog(State(app): State<Arc<App>>) -> Result<Json<CatalogView>, Error> {
    Ok(Json(app.snapshot().await?.catalog))
}
async fn workers(State(app): State<Arc<App>>) -> Result<Json<Vec<WorkerSummaryView>>, Error> {
    let mut workers = Vec::new();
    for worker in app.snapshot().await?.workers {
        let mut committed = BTreeMap::<Week, Minutes>::new();
        for reservation in &worker.calendar.reservations {
            let total = committed.entry(reservation.week).or_insert(Minutes::ZERO);
            *total = total
                .checked_add(reservation.minutes)
                .map_err(|_| Error::Dependency)?;
        }
        workers.push(WorkerSummaryView {
            account: worker.account,
            display_name: worker.display_name,
            qualifications: worker.qualifications,
            mode: worker.mode,
            coordinator_eligible: worker.coordinator_eligible,
            worker_score: worker.worker_score,
            coordinator_score: worker.coordinator_score,
            calendar: CalendarSummaryView {
                calendar_id: worker.calendar.calendar_id,
                owner: worker.calendar.owner,
                definition: worker.calendar.definition,
                committed_minutes: committed
                    .into_iter()
                    .map(|(week, minutes)| WeeklyCommitment { week, minutes })
                    .collect(),
            },
        });
    }
    Ok(Json(workers))
}
async fn projects(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<Vec<ProjectView>>, Error> {
    Ok(Json(
        app.snapshot()
            .await?
            .projects
            .into_iter()
            .filter(|p| operations::visible(p, session.view.account_id))
            .collect(),
    ))
}
async fn project(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<ProjectView>, Error> {
    let id = parse::<EntityId>(&id)?;
    Ok(Json(
        app.snapshot()
            .await?
            .projects
            .into_iter()
            .find(|p| p.project_id == id && operations::visible(p, session.view.account_id))
            .ok_or(Error::NotFound)?,
    ))
}

async fn visible_storage(
    app: &App,
    session: &Session,
    storage_id: EntityId,
) -> Result<(EntityId, TaskStorageView), Error> {
    app.snapshot()
        .await?
        .projects
        .into_iter()
        .filter(|project| operations::visible(project, session.view.account_id))
        .find_map(|project| {
            project
                .proposals
                .into_iter()
                .flat_map(|proposal| proposal.milestones)
                .find(|milestone| milestone.task_storage.task_storage_id == storage_id)
                .map(|milestone| (project.project_id, milestone.task_storage))
        })
        .ok_or(Error::NotFound)
}

async fn task_storage(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(storage_id): Path<String>,
) -> Result<Json<TaskStorageView>, Error> {
    let (_, storage) = visible_storage(&app, &session, parse(&storage_id)?).await?;
    Ok(Json(storage))
}

async fn task(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path((storage_id, task_id)): Path<(String, String)>,
) -> Result<Json<TaskView>, Error> {
    let (_, storage) = visible_storage(&app, &session, parse(&storage_id)?).await?;
    let task_id = parse::<u32>(&task_id)?;
    Ok(Json(
        storage
            .tasks
            .into_iter()
            .find(|task| task.task_id == task_id)
            .ok_or(Error::NotFound)?,
    ))
}
async fn balance(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<BalanceView>, Error> {
    Ok(Json(
        app.snapshot()
            .await?
            .balances
            .into_iter()
            .find(|b| b.account == session.view.account_id)
            .unwrap_or(BalanceView {
                account: session.view.account_id,
                asset_id: 1,
                available: Money::ZERO,
            }),
    ))
}
async fn operation(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<OperationView>, Error> {
    Ok(Json(operations::read(&app, &session, parse(&id)?).await?))
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    #[serde(default)]
    after: u64,
}
async fn notification_page(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<NotificationsPage>, Error> {
    Ok(Json(
        notifications::page(&app, session.view.account_id, cursor.after).await?,
    ))
}
async fn mark_read(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<NotificationView>, Error> {
    Ok(Json(
        notifications::mark_read(&app, session.view.account_id, parse(&id)?).await?,
    ))
}
async fn events(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
) -> Result<impl IntoResponse, Error> {
    let after = match headers.get("last-event-id") {
        Some(value) => parse(value.to_str().map_err(|_| Error::Invalid)?)?,
        None => cursor.after,
    };
    notifications::stream(app, session, after)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn command(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    route: MatchedPath,
    Path(params): Path<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<OperationRef>), Error> {
    let id = |field: &str| -> Result<EntityId, Error> {
        parse(params.get(field).ok_or(Error::Invalid)?)
    };
    let no_body = || -> Result<(), Error> {
        if body.iter().all(u8::is_ascii_whitespace) {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    };
    let revision = || -> Result<u64, Error> {
        Ok(json::<RevisionRequest>(&headers, &body)?.expected_revision)
    };
    let action = match route.as_str() {
        "/api/workers" => ProviderCommand::RegisterWorker(json(&headers, &body)?),
        "/api/workers/me/qualifications" => {
            ProviderCommand::UpdateQualifications(json(&headers, &body)?)
        }
        "/api/workers/me/calendar" => ProviderCommand::SetCalendar(json(&headers, &body)?),
        "/api/workers/me/mode" => ProviderCommand::SetWorkerMode(json(&headers, &body)?),
        "/api/bramp/deposits" => ProviderCommand::CreateDeposit(json(&headers, &body)?),
        "/api/admin/bramp/deposits/{depositId}/confirm" => {
            no_body()?;
            ProviderCommand::ConfirmDeposit {
                deposit_id: id("depositId")?,
            }
        }
        "/api/bramp/withdrawals" => ProviderCommand::CreateWithdrawal(json(&headers, &body)?),
        "/api/bramp/withdrawals/{withdrawalId}/cancel" => {
            no_body()?;
            ProviderCommand::CancelWithdrawal {
                withdrawal_id: id("withdrawalId")?,
            }
        }
        "/api/projects" => ProviderCommand::CreateProject(json(&headers, &body)?),
        "/api/projects/{projectId}/evaluations" => ProviderCommand::EvaluateProject {
            project_id: id("projectId")?,
            request: json(&headers, &body)?,
        },
        "/api/projects/{projectId}/planning/quote" => ProviderCommand::QuotePlanning {
            project_id: id("projectId")?,
            quote: json(&headers, &body)?,
        },
        "/api/projects/{projectId}/planning/accept" => ProviderCommand::AcceptPlanningQuote {
            project_id: id("projectId")?,
            expected_revision: revision()?,
        },
        "/api/projects/{projectId}/planning/accept-delivery" => {
            ProviderCommand::AcceptPlanningDelivery {
                project_id: id("projectId")?,
                expected_revision: revision()?,
            }
        }
        "/api/projects/{projectId}/planning/dispute" => ProviderCommand::DisputePlanning {
            project_id: id("projectId")?,
            request: json(&headers, &body)?,
        },
        "/api/projects/{projectId}/proposals" => ProviderCommand::CreateProposal {
            project_id: id("projectId")?,
            proposal: json(&headers, &body)?,
        },
        "/api/projects/{projectId}/proposals/{proposalId}" if method == Method::PUT => {
            ProviderCommand::UpdateProposal {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
                proposal: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}" => {
            no_body()?;
            ProviderCommand::DeleteProposal {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}/delivery" => {
            ProviderCommand::SetProposalDelivery {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
                request: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}/withdraw" => {
            ProviderCommand::WithdrawProposal {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
                expected_revision: revision()?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}/submit" => {
            no_body()?;
            ProviderCommand::SubmitProposal {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}/approve" => {
            ProviderCommand::ApproveExecution {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
                expected_revision: revision()?,
            }
        }
        "/api/projects/{projectId}/proposals/{proposalId}/changes" => {
            ProviderCommand::RequestProposalChanges {
                project_id: id("projectId")?,
                proposal_id: id("proposalId")?,
                request: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/cancel" => ProviderCommand::CancelProject {
            project_id: id("projectId")?,
            request: json(&headers, &body)?,
        },
        "/api/projects/{projectId}/milestones/{milestoneId}/request-completion"
        | "/api/projects/{projectId}/milestones/{milestoneId}/completion-submissions" => {
            ProviderCommand::SubmitMilestoneDelivery {
                project_id: id("projectId")?,
                milestone_id: id("milestoneId")?,
                request: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/milestones/{milestoneId}/accept-completion" => {
            let request: generated_contracts::AcceptMilestoneDeliveryRequest =
                json(&headers, &body)?;
            ProviderCommand::AcceptMilestoneDelivery {
                project_id: id("projectId")?,
                milestone_id: id("milestoneId")?,
                submission_id: request.submission_id,
            }
        }
        "/api/disputes" => {
            crate::disputes::conversations::opening(
                &app,
                &session,
                json(&headers, &body)?,
                headers
                    .get("idempotency-key")
                    .ok_or(Error::Invalid)?
                    .to_str()
                    .map_err(|_| Error::Invalid)?,
            )
            .await?
        }
        "/api/completion-submissions/{submissionId}/rejection" => {
            crate::submission_comments::rejection(
                &app,
                &session,
                id("submissionId")?,
                json(&headers, &body)?,
            )
            .await?
        }
        "/api/disputes/{disputeId}/response" => {
            crate::disputes::response(&app, id("disputeId")?, json(&headers, &body)?).await?
        }
        "/api/projects/{projectId}/task-storages/{storageId}/tasks" => {
            ProviderCommand::CreateTask {
                project_id: id("projectId")?,
                task_storage_id: id("storageId")?,
                task: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}"
        | "/api/task-storages/{storageId}/tasks/{taskId}"
            if method == Method::DELETE =>
        {
            ProviderCommand::DeleteTask {
                project_id: if params.contains_key("projectId") {
                    id("projectId")?
                } else {
                    visible_storage(&app, &session, id("storageId")?).await?.0
                },
                task_storage_id: id("storageId")?,
                task_id: parse(params.get("taskId").ok_or(Error::Invalid)?)?,
                expected_revision: revision()?,
            }
        }
        "/api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}" => {
            ProviderCommand::EditTask {
                project_id: id("projectId")?,
                task_storage_id: id("storageId")?,
                task_id: parse(params.get("taskId").ok_or(Error::Invalid)?)?,
                task: json(&headers, &body)?,
            }
        }
        "/api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}/progress" => {
            ProviderCommand::UpdateTaskProgress {
                project_id: id("projectId")?,
                task_storage_id: id("storageId")?,
                task_id: parse(params.get("taskId").ok_or(Error::Invalid)?)?,
                progress: json(&headers, &body)?,
            }
        }
        "/api/task-storages/{storageId}/tasks" => ProviderCommand::CreateTask {
            project_id: visible_storage(&app, &session, id("storageId")?).await?.0,
            task_storage_id: id("storageId")?,
            task: json(&headers, &body)?,
        },
        "/api/task-storages/{storageId}/tasks/{taskId}" if method == Method::PUT => {
            ProviderCommand::EditTask {
                project_id: visible_storage(&app, &session, id("storageId")?).await?.0,
                task_storage_id: id("storageId")?,
                task_id: parse(params.get("taskId").ok_or(Error::Invalid)?)?,
                task: json(&headers, &body)?,
            }
        }
        "/api/task-storages/{storageId}/tasks/{taskId}" => ProviderCommand::UpdateTaskProgress {
            project_id: visible_storage(&app, &session, id("storageId")?).await?.0,
            task_storage_id: id("storageId")?,
            task_id: parse(params.get("taskId").ok_or(Error::Invalid)?)?,
            progress: json(&headers, &body)?,
        },
        "/api/admin/coordinators" => ProviderCommand::PromoteCoordinator(json(&headers, &body)?),
        "/api/catalog/skill-requests" => {
            ProviderCommand::CreateSkillRequest(json(&headers, &body)?)
        }
        "/api/admin/catalog/skill-requests/{requestId}/decision" => {
            ProviderCommand::DecideSkillRequest(generated_contracts::DecideSkillRequest {
                request_id: id("requestId")?,
                decision: json(&headers, &body)?,
            })
        }
        "/api/admin/catalog" => ProviderCommand::UpsertCatalogEntry(json(&headers, &body)?),
        "/api/admin/catalog/{kind}/{id}" => {
            no_body()?;
            ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
                kind: match params.get("kind").map(String::as_str) {
                    Some("Role") => CatalogKind::Role,
                    Some("Skill") => CatalogKind::Skill,
                    _ => return Err(Error::Invalid),
                },
                id: parse(params.get("id").ok_or(Error::Invalid)?)?,
            })
        }
        "/api/admin/score-policy" => ProviderCommand::SetScorePolicy(json(&headers, &body)?),
        "/api/admin/fund" if app.config.enable_mock_funding => {
            ProviderCommand::FundAccount(json(&headers, &body)?)
        }
        _ => return Err(Error::NotFound),
    };
    let key = headers
        .get("idempotency-key")
        .map(|value| value.to_str().map_err(|_| Error::Invalid).and_then(parse))
        .transpose()?;
    let operation = operations::enqueue(&app, &session, action, key).await?;
    tracing::info!(operation_id = %operation.operation_id, "authorized operation enqueued");
    Ok((StatusCode::ACCEPTED, Json(operation)))
}
