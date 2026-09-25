//! Server-side `WebAuthn` ceremonies for the adapter's existing principals.
mod store;

use super::{Session, create_session, ensure_wallet, username, verify_password};
use crate::state::{App, Error, now, parse, token};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::Response,
};
use generated_contracts::PrincipalId;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{env, sync::Arc};
use webauthn_rs::prelude::{
    CreationChallengeResponse, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Url, Uuid, Webauthn, WebauthnBuilder,
};
use zeroize::Zeroizing;

const CEREMONY_SECONDS: i64 = 300;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegisterOptionsRequest {
    current_password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CeremonyOptions<T> {
    ceremony_id: String,
    options: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegisterVerifyRequest {
    ceremony_id: String,
    credential: RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LoginVerifyRequest {
    ceremony_id: String,
    credential: PublicKeyCredential,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LoginOptionsRequest {
    username: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveRequest {
    current_password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PasskeyView {
    id: String,
    created_at: i64,
}

pub(crate) fn configured_webauthn() -> Result<Webauthn, Error> {
    let rp_id = env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| "localhost".to_owned());
    let origin = env::var("WEBAUTHN_ORIGIN").unwrap_or_else(|_| "http://localhost:8088".to_owned());
    let parsed = Url::parse(&origin).map_err(|_| Error::Config)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || (parsed.scheme() == "http" && parsed.host_str() != Some("localhost"))
        || parsed.origin().ascii_serialization() != origin
    {
        return Err(Error::Config);
    }
    WebauthnBuilder::new(&rp_id, &parsed)
        .map_err(|_| Error::Config)?
        .build()
        .map_err(|_| Error::Config)
}

async fn reauthenticate(app: &App, session: &Session, password: String) -> Result<(), Error> {
    let row = sqlx::query("SELECT password_hash FROM principals WHERE principal_id = $1")
        .bind(session.view.principal_id.to_string())
        .fetch_one(&app.db)
        .await?;
    let encoded: String = row.try_get("password_hash")?;
    if !verify_password(app, Zeroizing::new(password), encoded).await? {
        return Err(Error::Unauthorized);
    }
    Ok(())
}

pub(crate) async fn register_options(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Json(request): Json<RegisterOptionsRequest>,
) -> Result<Json<CeremonyOptions<CreationChallengeResponse>>, Error> {
    reauthenticate(&app, &session, request.current_password).await?;
    let principal = session.view.principal_id;
    let user_id = Uuid::new_v4().to_string();
    let row = sqlx::query("UPDATE principals SET passkey_user_id = COALESCE(passkey_user_id, $1) WHERE principal_id = $2 RETURNING passkey_user_id, username, display_name")
        .bind(user_id).bind(principal.to_string()).fetch_one(&app.db).await?;
    let id: String = row.try_get("passkey_user_id")?;
    let id = Uuid::parse_str(&id).map_err(|_| Error::Internal)?;
    let username: String = row.try_get("username")?;
    let display_name: String = row.try_get("display_name")?;
    let existing = store::credentials(&app.db, principal).await?;
    let excluded = existing.iter().map(|key| key.cred_id().clone()).collect();
    let (options, state) = configured_webauthn()?
        .start_passkey_registration(id, &username, &display_name, Some(excluded))
        .map_err(|_| Error::Invalid)?;
    let ceremony_id = token()?.to_string();
    store::insert_ceremony(
        &app.db,
        &ceremony_id,
        principal,
        "register",
        &state,
        Some(&session.token_hash),
        now()? + CEREMONY_SECONDS,
    )
    .await?;
    Ok(Json(CeremonyOptions {
        ceremony_id,
        options,
    }))
}

pub(crate) async fn register_verify(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Json(request): Json<RegisterVerifyRequest>,
) -> Result<StatusCode, Error> {
    let state = store::consume_ceremony(
        &app.db,
        &request.ceremony_id,
        "register",
        Some(session.view.principal_id),
        Some(&session.token_hash),
    )
    .await?;
    let state = serde_json::from_str(&state)?;
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    let passkey = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        configured_webauthn()?
            .finish_passkey_registration(&request.credential, &state)
            .map_err(|_| Error::Unauthorized)
    })
    .await??;
    store::insert_credential(&app.db, session.view.principal_id, &passkey).await?;
    app.audit(
        Some(&session.view.principal_id.to_string()),
        None,
        "passkey_registered",
        "ok",
    )
    .await?;
    Ok(StatusCode::CREATED)
}

pub(crate) async fn list(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
) -> Result<Json<Vec<PasskeyView>>, Error> {
    let rows = sqlx::query("SELECT credential_ref, created_at FROM passkey_credentials WHERE principal_id = $1 ORDER BY created_at, credential_ref")
        .bind(session.view.principal_id.to_string()).fetch_all(&app.db).await?;
    rows.into_iter()
        .map(|row| {
            Ok(PasskeyView {
                id: row.try_get("credential_ref")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()
        .map(Json)
}

pub(crate) async fn remove(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(credential_ref): Path<String>,
    Json(request): Json<RemoveRequest>,
) -> Result<StatusCode, Error> {
    reauthenticate(&app, &session, request.current_password).await?;
    let result = sqlx::query(
        "DELETE FROM passkey_credentials WHERE credential_ref = $1 AND principal_id = $2",
    )
    .bind(credential_ref)
    .bind(session.view.principal_id.to_string())
    .execute(&app.db)
    .await?;
    if result.rows_affected() != 1 {
        return Err(Error::NotFound);
    }
    app.audit(
        Some(&session.view.principal_id.to_string()),
        None,
        "passkey_removed",
        "ok",
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn login_options(
    State(app): State<Arc<App>>,
    Json(request): Json<LoginOptionsRequest>,
) -> Result<Json<CeremonyOptions<RequestChallengeResponse>>, Error> {
    let name = username(&request.username).map_err(|_| Error::Unauthorized)?;
    let row = sqlx::query("SELECT principal_id FROM principals WHERE username = $1")
        .bind(name)
        .fetch_optional(&app.db)
        .await?
        .ok_or(Error::Unauthorized)?;
    let principal: PrincipalId = parse(row.try_get("principal_id")?)?;
    begin_login(&app, principal).await.map(Json)
}

pub(crate) async fn begin_login(
    app: &App,
    principal: PrincipalId,
) -> Result<CeremonyOptions<RequestChallengeResponse>, Error> {
    let credentials = store::credentials(&app.db, principal).await?;
    if credentials.is_empty() {
        return Err(Error::Unauthorized);
    }
    let (options, state) = configured_webauthn()?
        .start_passkey_authentication(&credentials)
        .map_err(|_| Error::Unauthorized)?;
    let ceremony_id = token()?.to_string();
    store::insert_ceremony(
        &app.db,
        &ceremony_id,
        principal,
        "login",
        &state,
        None,
        now()? + CEREMONY_SECONDS,
    )
    .await?;
    Ok(CeremonyOptions {
        ceremony_id,
        options,
    })
}

pub(crate) async fn login_verify(
    State(app): State<Arc<App>>,
    Json(request): Json<LoginVerifyRequest>,
) -> Result<Response, Error> {
    let (principal, state) = store::consume_login(&app.db, &request.ceremony_id).await?;
    let state = serde_json::from_str(&state)?;
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        configured_webauthn()?
            .finish_passkey_authentication(&request.credential, &state)
            .map_err(|_| Error::Unauthorized)
    })
    .await??;
    if !result.user_verified() {
        return Err(Error::Unauthorized);
    }
    store::update_credential(&app.db, principal, &result).await?;
    let wallet = ensure_wallet(&app, principal).await?;
    app.audit(Some(&principal.to_string()), None, "passkey_login", "ok")
        .await?;
    create_session(&app, principal, wallet, StatusCode::OK).await
}

#[cfg(test)]
mod tests;
