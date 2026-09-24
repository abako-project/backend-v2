use crate::state::{App, Error, hash, now, parse, random, token};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use generated_contracts::{
    AccountId32, ChangePasswordRequest, LoginRequest, PrincipalId, ProvisionWalletRequest,
    RegisterRequest, SessionView, WalletId, WalletLifecycle, WalletView,
};
use sqlx::{Row, postgres::PgRow};
use std::sync::Arc;
use zeroize::Zeroizing;

pub(crate) mod passkeys;

const SESSION_SECONDS: i64 = 86_400;

#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) view: SessionView,
    pub(crate) wallet: WalletId,
    pub(crate) token_hash: [u8; 32],
}

fn username(value: &str) -> Result<String, Error> {
    if !(3..=64).contains(&value.len())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::Invalid);
    }
    Ok(value.to_ascii_lowercase())
}
fn password(value: &str) -> Result<(), Error> {
    if !(12..=1024).contains(&value.len()) {
        return Err(Error::Invalid);
    }
    Ok(())
}
pub(crate) async fn hash_password(app: &App, secret: Zeroizing<String>) -> Result<String, Error> {
    password(&secret)?;
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let salt = SaltString::encode_b64(&random::<16>()?).map_err(|_| Error::Internal)?;
        Argon2::default()
            .hash_password(secret.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| Error::Internal)
    })
    .await?
}
async fn verify_password(
    app: &App,
    secret: Zeroizing<String>,
    encoded: String,
) -> Result<bool, Error> {
    if secret.len() > 1024 {
        return Ok(false);
    }
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let parsed = PasswordHash::new(&encoded).map_err(|_| Error::Internal)?;
        Ok(Argon2::default()
            .verify_password(secret.as_bytes(), &parsed)
            .is_ok())
    })
    .await?
}

pub(crate) async fn bootstrap(app: &Arc<App>) -> Result<(), Error> {
    let name = username(&app.config.admin_username).map_err(|_| Error::Config)?;
    let existing = sqlx::query("SELECT is_admin FROM principals WHERE username = $1")
        .bind(&name)
        .fetch_optional(&app.db)
        .await?;
    if let Some(row) = existing {
        if !row.try_get::<bool, _>("is_admin")? {
            return Err(Error::Config);
        }
        return Ok(());
    }
    let encoded = hash_password(app, app.config.admin_password.clone()).await?;
    let id = PrincipalId::from_bytes(random()?);
    sqlx::query("INSERT INTO principals(principal_id, username, password_hash, display_name, is_admin, created_at) VALUES ($1, $2, $3, 'Administrator', TRUE, $4) ON CONFLICT(username) DO NOTHING")
        .bind(id.to_string()).bind(&name).bind(encoded).bind(now()?).execute(&app.db).await?;
    let row = sqlx::query("SELECT is_admin FROM principals WHERE username = $1")
        .bind(&name)
        .fetch_one(&app.db)
        .await?;
    if !row.try_get::<bool, _>("is_admin")? {
        return Err(Error::Config);
    }
    app.audit(
        Some(&id.to_string()),
        None,
        "administrator_bootstrapped",
        "ok",
    )
    .await
}

pub(crate) async fn register(app: &App, request: RegisterRequest) -> Result<Response, Error> {
    let RegisterRequest {
        username: name,
        password: secret,
        display_name,
    } = request;
    let name = username(&name)?;
    if name == username(&app.config.admin_username)? {
        return Err(Error::Conflict("username_taken"));
    }
    if display_name.trim().is_empty() || display_name.len() > 128 {
        return Err(Error::Invalid);
    }
    let encoded = hash_password(app, Zeroizing::new(secret)).await?;
    let id = PrincipalId::from_bytes(random()?);
    let result = sqlx::query("INSERT INTO principals(principal_id, username, password_hash, display_name, is_admin, created_at) VALUES ($1, $2, $3, $4, FALSE, $5) ON CONFLICT(username) DO NOTHING")
        .bind(id.to_string()).bind(&name).bind(encoded).bind(display_name).bind(now()?).execute(&app.db).await?;
    if result.rows_affected() == 0 {
        return Err(Error::Conflict("username_taken"));
    }
    // Persist the principal first. Login retries idempotent provisioning after a lost custody reply.
    let wallet = ensure_wallet(app, id).await?;
    app.audit(Some(&id.to_string()), None, "principal_registered", "ok")
        .await?;
    create_session(app, id, wallet, StatusCode::CREATED).await
}

pub(crate) async fn login(app: &App, request: LoginRequest) -> Result<Response, Error> {
    let LoginRequest {
        username: name,
        password: secret,
    } = request;
    let name = username(&name).map_err(|_| Error::Unauthorized)?;
    let row = sqlx::query("SELECT principal_id, password_hash FROM principals WHERE username = $1")
        .bind(&name)
        .fetch_optional(&app.db)
        .await?;
    let encoded = if let Some(row) = &row {
        row.try_get("password_hash")?
    } else {
        // Equal-cost verification avoids a cheap username-existence timing oracle.
        let row = sqlx::query("SELECT password_hash FROM principals WHERE is_admin = TRUE LIMIT 1")
            .fetch_one(&app.db)
            .await?;
        row.try_get("password_hash")?
    };
    let valid = verify_password(app, Zeroizing::new(secret), encoded).await?;
    let Some(row) = row.filter(|_| valid) else {
        app.audit(None, None, "login_denied", "invalid_credentials")
            .await?;
        return Err(Error::Unauthorized);
    };
    let id: PrincipalId = parse(row.try_get("principal_id")?)?;
    let wallet = ensure_wallet(app, id).await?;
    app.audit(Some(&id.to_string()), None, "login", "ok")
        .await?;
    create_session(app, id, wallet, StatusCode::OK).await
}

async fn ensure_wallet(app: &App, id: PrincipalId) -> Result<WalletView, Error> {
    let wallet: WalletView = app
        .post(
            &app.config.custody_url,
            "/internal/wallets",
            &ProvisionWalletRequest { principal_id: id },
        )
        .await?;
    if wallet.principal_id != Some(id) || wallet.lifecycle != WalletLifecycle::Active {
        return Err(Error::Dependency);
    }
    let affected = sqlx::query("UPDATE principals SET wallet_id = $1, account_id = $2 WHERE principal_id = $3 AND (wallet_id IS NULL OR (wallet_id = $4 AND account_id = $5))")
        .bind(wallet.wallet_id.to_string()).bind(wallet.account_id.to_string()).bind(id.to_string())
        .bind(wallet.wallet_id.to_string()).bind(wallet.account_id.to_string()).execute(&app.db).await?;
    if affected.rows_affected() != 1 {
        return Err(Error::Dependency);
    }
    Ok(wallet)
}

fn view(row: &PgRow) -> Result<SessionView, Error> {
    Ok(SessionView {
        principal_id: parse(row.try_get("principal_id")?)?,
        account_id: parse(row.try_get("account_id")?)?,
        display_name: row.try_get("display_name")?,
        csrf_token: row.try_get("csrf_token")?,
        is_admin: row.try_get("is_admin")?,
    })
}
fn cookie(value: &str, secure: bool, seconds: i64) -> Result<HeaderValue, Error> {
    HeaderValue::from_str(&format!(
        "kunveno_session={value}; HttpOnly; SameSite=Lax; Path=/api; Max-Age={seconds}{}",
        if secure { "; Secure" } else { "" }
    ))
    .map_err(|_| Error::Internal)
}
async fn create_session(
    app: &App,
    id: PrincipalId,
    wallet: WalletView,
    status: StatusCode,
) -> Result<Response, Error> {
    let session_token = token()?;
    let csrf_token = token()?;
    let time = now()?;
    let mut transaction = app.db.begin().await?;
    sqlx::query("SELECT principal_id FROM principals WHERE principal_id = $1 FOR UPDATE")
        .bind(id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(time)
        .execute(&mut *transaction)
        .await?;
    // Bound sessions per principal without revoking the most recent active clients.
    sqlx::query("DELETE FROM sessions WHERE principal_id = $1 AND token_hash NOT IN (SELECT token_hash FROM sessions WHERE principal_id = $2 ORDER BY expires_at DESC LIMIT 15)")
        .bind(id.to_string()).bind(id.to_string()).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO sessions(token_hash, principal_id, csrf_token, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(hash(session_token.as_bytes()).to_vec()).bind(id.to_string()).bind(csrf_token.as_str()).bind(time + SESSION_SECONDS).execute(&mut *transaction).await?;
    let row = sqlx::query("SELECT display_name, is_admin FROM principals WHERE principal_id = $1")
        .bind(id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
    let response = SessionView {
        principal_id: id,
        account_id: wallet.account_id,
        display_name: row.try_get("display_name")?,
        csrf_token: csrf_token.to_string(),
        is_admin: row.try_get("is_admin")?,
    };
    transaction.commit().await?;
    Ok((
        status,
        [(
            header::SET_COOKIE,
            cookie(&session_token, app.config.cookie_secure, SESSION_SECONDS)?,
        )],
        Json(response),
    )
        .into_response())
}

pub(crate) async fn authenticate(app: &App, headers: &HeaderMap) -> Result<Session, Error> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|part| part.trim().strip_prefix("kunveno_session="));
    let token = values.next().ok_or(Error::Unauthorized)?;
    if values.next().is_some() || token.len() != 66 || parse::<AccountId32>(token).is_err() {
        return Err(Error::Unauthorized);
    }
    authenticate_hash(app, hash(token.as_bytes())).await
}
pub(crate) async fn authenticate_hash(app: &App, token_hash: [u8; 32]) -> Result<Session, Error> {
    let row = sqlx::query("SELECT p.principal_id, p.account_id, p.wallet_id, p.display_name, p.is_admin, s.csrf_token FROM sessions s JOIN principals p USING(principal_id) WHERE token_hash = $1 AND expires_at > $2")
        .bind(token_hash.to_vec()).bind(now()?).fetch_optional(&app.db).await?.ok_or(Error::Unauthorized)?;
    Ok(Session {
        view: view(&row)?,
        wallet: parse(row.try_get("wallet_id")?)?,
        token_hash,
    })
}
pub(crate) fn valid_csrf(session: &Session, headers: &HeaderMap) -> bool {
    headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| hash(value.as_bytes()) == hash(session.view.csrf_token.as_bytes()))
}
pub(crate) async fn logout(app: &App, session: &Session) -> Result<Response, Error> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(session.token_hash.to_vec())
        .execute(&app.db)
        .await?;
    app.audit(
        Some(&session.view.principal_id.to_string()),
        None,
        "logout",
        "ok",
    )
    .await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie("", app.config.cookie_secure, 0)?)],
    )
        .into_response())
}
pub(crate) async fn change_password(
    app: &App,
    session: &Session,
    request: ChangePasswordRequest,
) -> Result<StatusCode, Error> {
    let ChangePasswordRequest {
        current_password,
        new_password,
    } = request;
    let row = sqlx::query("SELECT password_hash FROM principals WHERE principal_id = $1")
        .bind(session.view.principal_id.to_string())
        .fetch_one(&app.db)
        .await?;
    let old: String = row.try_get("password_hash")?;
    if !verify_password(app, Zeroizing::new(current_password), old.clone()).await? {
        return Err(Error::Unauthorized);
    }
    let replacement = hash_password(app, Zeroizing::new(new_password)).await?;
    let mut transaction = app.db.begin().await?;
    let result = sqlx::query(
        "UPDATE principals SET password_hash = $1 WHERE principal_id = $2 AND password_hash = $3",
    )
    .bind(replacement)
    .bind(session.view.principal_id.to_string())
    .bind(old)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(Error::Conflict("credentials_changed"));
    }
    sqlx::query("DELETE FROM sessions WHERE principal_id = $1 AND token_hash != $2")
        .bind(session.view.principal_id.to_string())
        .bind(session.token_hash.to_vec())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    app.audit(
        Some(&session.view.principal_id.to_string()),
        None,
        "password_changed",
        "ok",
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
