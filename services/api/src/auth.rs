use crate::state::{App, Config, Error, hash, now, parse, random, token};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chain_provider::Key;
use protocol::{
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
/// What `password_hash` holds when a password is the device of a pass
/// account: the salt of the derivation and the device's public key, in hex.
/// It holds no hash of the password and no seed.
pub(crate) const DEVICE_PREFIX: &str = "$pass-device$v=1$";
/// The device key of `password`: an sr25519 seed from Argon2id, keyed with
/// the deployment's derivation secret.
fn derive_device(secret: &[u8], password: &[u8], salt: &[u8; 16]) -> Result<Key, Error> {
    let mut seed = Zeroizing::new([0_u8; 32]);
    Argon2::new_with_secret(
        secret,
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        // Version 1 of the stored record: 19 MiB, two passes, one lane, 32
        // bytes. A change of these changes every device, so they are not
        // left to the library's defaults.
        argon2::Params::new(19_456, 2, 1, Some(32)).map_err(|_| Error::Internal)?,
    )
    .map_err(|_| Error::Internal)?
    .hash_password_into(password, salt, &mut seed[..])
    .map_err(|_| Error::Internal)?;
    Key::from_seed(&seed).map_err(|_| Error::Internal)
}
fn derivation_secret(config: &Config) -> Result<Zeroizing<String>, Error> {
    Ok(config
        .chain
        .as_ref()
        .ok_or(Error::Internal)?
        .derivation_secret
        .clone())
}
/// What is stored for the device `device` derived with `salt`.
fn device_record(salt: [u8; 16], device: &Key) -> String {
    format!(
        "{DEVICE_PREFIX}{}${}",
        protocol::EntityId::from_bytes(salt),
        device.account()
    )
}
/// The salt and public key a stored record names, if it is a device's.
fn parse_device(encoded: &str) -> Option<([u8; 16], AccountId32)> {
    let (salt, public) = encoded.strip_prefix(DEVICE_PREFIX)?.split_once('$')?;
    Some((
        parse::<protocol::EntityId>(salt).ok()?.into_bytes(),
        parse(public).ok()?,
    ))
}
/// The administrator's principal and its device, before the chain starts:
/// the administrator's pass account is the Dao's operator, and a first start
/// registers it with this device. Returns no device when the stored record
/// no longer matches the bootstrap password, which only a resumed chain can
/// live with.
pub(crate) async fn bootstrap_device(
    db: &sqlx::PgPool,
    config: &Config,
) -> Result<(Option<Key>, bool), Error> {
    let name = username(&config.admin_username).map_err(|_| Error::Config)?;
    let secret = derivation_secret(config)?;
    let password = config.admin_password.clone();
    let existing =
        sqlx::query("SELECT password_hash, is_admin FROM principals WHERE username = $1")
            .bind(&name)
            .fetch_optional(db)
            .await?;
    if let Some(row) = existing {
        // A record from the service mode is not a device: refuse to start.
        let stored: String = row.try_get("password_hash")?;
        let (salt, public) = parse_device(&stored).ok_or(Error::Config)?;
        if !row.try_get::<bool, _>("is_admin")? {
            return Err(Error::Config);
        }
        let device = tokio::task::spawn_blocking(move || {
            derive_device(secret.as_bytes(), password.as_bytes(), &salt)
        })
        .await??;
        return Ok(((device.account() == public).then_some(device), false));
    }
    self::password(&password).map_err(|_| Error::Config)?;
    let salt = random::<16>()?;
    let device = tokio::task::spawn_blocking(move || {
        derive_device(secret.as_bytes(), password.as_bytes(), &salt)
    })
    .await??;
    sqlx::query("INSERT INTO principals(principal_id, username, password_hash, display_name, is_admin, created_at) VALUES ($1, $2, $3, 'Administrator', TRUE, $4)")
        .bind(PrincipalId::from_bytes(random()?).to_string())
        .bind(&name)
        .bind(device_record(salt, &device))
        .bind(now()?)
        .execute(db)
        .await?;
    Ok((Some(device), true))
}
/// A new salt and the device of `secret` under it, with what is stored.
pub(crate) async fn new_device(
    app: &App,
    secret: Zeroizing<String>,
) -> Result<(String, Key), Error> {
    password(&secret)?;
    let key = derivation_secret(&app.config)?;
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let salt = random::<16>()?;
        let device = derive_device(key.as_bytes(), secret.as_bytes(), &salt)?;
        Ok((device_record(salt, &device), device))
    })
    .await?
}
/// The device of `secret` with the salt `salt`, whatever `encoded` names.
async fn device_with(app: &App, secret: Zeroizing<String>, salt: [u8; 16]) -> Result<Key, Error> {
    let key = derivation_secret(&app.config)?;
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        derive_device(key.as_bytes(), secret.as_bytes(), &salt)
    })
    .await?
}
/// The device of `secret`, if it is the one `encoded` names.
pub(crate) async fn unlock_device(
    app: &App,
    secret: Zeroizing<String>,
    encoded: &str,
) -> Result<Option<Key>, Error> {
    // A record that is not a device's matches no password.
    let Some((salt, public)) = parse_device(encoded) else {
        return Ok(None);
    };
    if secret.len() > 1024 {
        return Ok(None);
    }
    let device = device_with(app, secret, salt).await?;
    Ok((device.account() == public).then_some(device))
}
/// What the chain's refusal means to a client.
pub(crate) fn chain_error(error: chain_provider::Error) -> Error {
    match error {
        chain_provider::Error::Provider(error) if error.code() == "username_taken" => {
            Error::Conflict("username_taken")
        }
        chain_provider::Error::Provider(error) if error.code() == "invalid_credentials" => {
            Error::Unauthorized
        }
        chain_provider::Error::Provider(error) if error.code() == "capacity_exceeded" => {
            Error::Capacity
        }
        error => {
            tracing::warn!(%error, "the chain did not answer an account request");
            Error::Dependency
        }
    }
}
/// Make `name` a pass account with `device` if it is not one yet, and open
/// its session. Stands where the wallet's provisioning stands otherwise.
async fn ensure_account(
    app: &App,
    chain: &chain_provider::Provider,
    id: PrincipalId,
    name: &str,
    device: &Key,
) -> Result<WalletView, Error> {
    let registered: Option<String> =
        sqlx::query_scalar("SELECT account_id FROM principals WHERE principal_id = $1")
            .bind(id.to_string())
            .fetch_one(&app.db)
            .await?;
    if registered.is_none() {
        match chain.register(name, device).await.map_err(chain_error) {
            // Registered by an earlier attempt whose answer was lost, or by
            // someone else: only this device's session tells them apart.
            Ok(_) | Err(Error::Conflict("username_taken")) => {}
            Err(error) => return Err(error),
        }
    }
    let (account, _) = match chain.open_session(name, device).await.map_err(chain_error) {
        Err(Error::Unauthorized) if registered.is_none() => {
            return Err(Error::Conflict("username_taken"));
        }
        other => other?,
    };
    let wallet = WalletView {
        // The operator's account is the wallet the privileged commands are
        // queued under: its own commands share that queue.
        wallet_id: if account == chain.operator() {
            chain.system_wallet()
        } else {
            WalletId::from_bytes(id.into_bytes())
        },
        principal_id: Some(id),
        account_id: account,
        lifecycle: WalletLifecycle::Active,
    };
    if registered.is_none() {
        sqlx::query("UPDATE principals SET wallet_id = $1, account_id = $2 WHERE principal_id = $3 AND wallet_id IS NULL")
            .bind(wallet.wallet_id.to_string()).bind(account.to_string()).bind(id.to_string())
            .execute(&app.db).await?;
    }
    Ok(wallet)
}

pub(crate) async fn hash_password(app: &App, secret: Zeroizing<String>) -> Result<String, Error> {
    if app.chain.is_some() {
        return Ok(new_device(app, secret).await?.0);
    }
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
    if encoded.starts_with(DEVICE_PREFIX) {
        return Ok(unlock_device(app, secret, &encoded).await?.is_some());
    }
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
    let secret = Zeroizing::new(secret);
    let (encoded, device) = if app.chain.is_some() {
        let (encoded, device) = new_device(app, secret).await?;
        (encoded, Some(device))
    } else {
        (hash_password(app, secret).await?, None)
    };
    let id = PrincipalId::from_bytes(random()?);
    let result = sqlx::query("INSERT INTO principals(principal_id, username, password_hash, display_name, is_admin, created_at) VALUES ($1, $2, $3, $4, FALSE, $5) ON CONFLICT(username) DO NOTHING")
        .bind(id.to_string()).bind(&name).bind(encoded).bind(display_name).bind(now()?).execute(&app.db).await?;
    if result.rows_affected() == 0 {
        return Err(Error::Conflict("username_taken"));
    }
    if let (Some(chain), Some(device)) = (&app.chain, &device) {
        let wallet = match ensure_account(app, chain, id, &name, device).await {
            // The username is someone else's pass account: it is not ours to keep.
            Err(Error::Conflict(code)) => {
                sqlx::query(
                    "DELETE FROM principals WHERE principal_id = $1 AND account_id IS NULL",
                )
                .bind(id.to_string())
                .execute(&app.db)
                .await?;
                return Err(Error::Conflict(code));
            }
            other => other?,
        };
        app.audit(Some(&id.to_string()), None, "principal_registered", "ok")
            .await?;
        return create_session(app, id, wallet, StatusCode::CREATED).await;
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
    if let Some(chain) = &app.chain {
        return device_login(app, chain, &name, Zeroizing::new(secret)).await;
    }
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

/// Settle a password change whose answer was lost, and return the record
/// that stands. The chain says which device the account has: no password and
/// no open session is taken as evidence. A change still on its way holds the
/// lock this waits for, so the chain is never asked too early.
pub(crate) async fn settle(
    app: &App,
    chain: &chain_provider::Provider,
    id: &str,
    name: &str,
    current: String,
    next: Option<String>,
    keep: Option<&[u8; 32]>,
) -> Result<String, Error> {
    if next.is_none() {
        return Ok(current);
    }
    let _held = app.credentials.lock().await;
    let row = sqlx::query(
        "SELECT password_hash, password_hash_next FROM principals WHERE principal_id = $1",
    )
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    let current: String = row.try_get("password_hash")?;
    let Some(next) = row.try_get::<Option<String>, _>("password_hash_next")? else {
        return Ok(current);
    };
    let changed = match parse_device(&next) {
        Some((_, device)) => chain.has_device(name, device).await.map_err(chain_error)?,
        None => false,
    };
    if !changed {
        sqlx::query("UPDATE principals SET password_hash_next = NULL WHERE principal_id = $1 AND password_hash_next = $2")
            .bind(id).bind(&next).execute(&app.db).await?;
        return Ok(current);
    }
    // The change went through. It was meant to end the other sessions, and
    // which one asked for it is no longer known: all end but the one that
    // is asking now, if one is.
    let mut transaction = app.db.begin().await?;
    let promoted = sqlx::query("UPDATE principals SET password_hash = $2, password_hash_next = NULL WHERE principal_id = $1 AND password_hash_next = $2")
        .bind(id).bind(&next).execute(&mut *transaction).await?;
    if promoted.rows_affected() != 1 {
        return Err(Error::Conflict("credentials_changed"));
    }
    sqlx::query(
        "DELETE FROM sessions WHERE principal_id = $1 AND ($2::BYTEA IS NULL OR token_hash != $2)",
    )
    .bind(id)
    .bind(keep.map(|hash| hash.to_vec()))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(next)
}

/// Login when a password is the device of a pass account: the device the
/// password gives opens the session, and the chain has the last word.
async fn device_login(
    app: &App,
    chain: &chain_provider::Provider,
    name: &str,
    secret: Zeroizing<String>,
) -> Result<Response, Error> {
    let row = sqlx::query(
        "SELECT principal_id, password_hash, password_hash_next FROM principals WHERE username = $1",
    )
    .bind(name)
    .fetch_optional(&app.db)
    .await?;
    let stored: String = if let Some(row) = &row {
        let id: String = row.try_get("principal_id")?;
        settle(
            app,
            chain,
            &id,
            name,
            row.try_get("password_hash")?,
            row.try_get("password_hash_next")?,
            None,
        )
        .await?
    } else {
        // Equal-cost derivation avoids a cheap username-existence timing oracle.
        sqlx::query_scalar("SELECT password_hash FROM principals WHERE is_admin = TRUE LIMIT 1")
            .fetch_one(&app.db)
            .await?
    };
    let device = unlock_device(app, secret, &stored).await?;
    let (Some(row), Some(device)) = (row, device) else {
        app.audit(None, None, "login_denied", "invalid_credentials")
            .await?;
        return Err(Error::Unauthorized);
    };
    let id: PrincipalId = parse(row.try_get("principal_id")?)?;
    let wallet = ensure_account(app, chain, id, name, &device).await?;
    app.audit(Some(&id.to_string()), None, "login", "ok")
        .await?;
    // A password change may have gone through since the record was read.
    create_session_for(app, id, wallet, StatusCode::OK, Some(&stored)).await
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
        "digesta_session={value}; HttpOnly; SameSite=Lax; Path=/api; Max-Age={seconds}{}",
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
    create_session_for(app, id, wallet, status, None).await
}

/// Create a session. With `unlocked`, the password record the login was
/// checked against, the session is refused if that record no longer stands.
async fn create_session_for(
    app: &App,
    id: PrincipalId,
    wallet: WalletView,
    status: StatusCode,
    unlocked: Option<&str>,
) -> Result<Response, Error> {
    let session_token = token()?;
    let csrf_token = token()?;
    let time = now()?;
    // A session lasts as long as the chain lets its key sign.
    let lifetime = match &app.chain {
        Some(chain) => {
            let until = chain
                .session_until(wallet.account_id)
                .ok_or(Error::Unauthorized)?;
            i64::try_from(until.get())
                .map_err(|_| Error::Internal)?
                .saturating_sub(time)
                .max(0)
        }
        None => SESSION_SECONDS,
    };
    let mut transaction = app.db.begin().await?;
    let standing: String = sqlx::query_scalar(
        "SELECT password_hash FROM principals WHERE principal_id = $1 FOR UPDATE",
    )
    .bind(id.to_string())
    .fetch_one(&mut *transaction)
    .await?;
    if unlocked.is_some_and(|record| record != standing) {
        return Err(Error::Unauthorized);
    }
    sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
        .bind(time)
        .execute(&mut *transaction)
        .await?;
    // Bound sessions per principal without revoking the most recent active clients.
    sqlx::query("DELETE FROM sessions WHERE principal_id = $1 AND token_hash NOT IN (SELECT token_hash FROM sessions WHERE principal_id = $2 ORDER BY expires_at DESC LIMIT 15)")
        .bind(id.to_string()).bind(id.to_string()).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO sessions(token_hash, principal_id, csrf_token, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(hash(session_token.as_bytes()).to_vec()).bind(id.to_string()).bind(csrf_token.as_str()).bind(time + lifetime).execute(&mut *transaction).await?;
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
            cookie(&session_token, app.config.cookie_secure, lifetime)?,
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
        .filter_map(|part| part.trim().strip_prefix("digesta_session="));
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
/// Claim the record of the new device, replace the device on the chain and
/// promote the record. Whoever calls this holds the credentials lock.
#[allow(clippy::too_many_arguments)]
async fn replace_device(
    db: &sqlx::PgPool,
    chain: &chain_provider::Provider,
    id: &str,
    name: &str,
    stored: &str,
    next: &str,
    old: &Key,
    new: &Key,
    keep: Vec<u8>,
) -> Result<(), Error> {
    let claimed = sqlx::query("UPDATE principals SET password_hash_next = $1 WHERE principal_id = $2 AND password_hash = $3 AND password_hash_next IS NULL")
        .bind(next).bind(id).bind(stored).execute(db).await?;
    if claimed.rows_affected() != 1 {
        return Err(Error::Conflict("credentials_changed"));
    }
    match chain
        .replace_device(name, old, new)
        .await
        .map_err(chain_error)
    {
        Ok(()) => {}
        // The chain refused the old device: nothing changed.
        Err(Error::Unauthorized) => {
            sqlx::query("UPDATE principals SET password_hash_next = NULL WHERE principal_id = $1 AND password_hash_next = $2")
                .bind(id).bind(next).execute(db).await?;
            return Err(Error::Unauthorized);
        }
        // Whether the device changed is not known. The record of the new
        // one stays, and the next login or change settles it from the chain.
        Err(error) => return Err(error),
    }
    let mut transaction = db.begin().await?;
    sqlx::query("UPDATE principals SET password_hash = $2, password_hash_next = NULL WHERE principal_id = $1 AND password_hash_next = $2")
        .bind(id).bind(next).execute(&mut *transaction).await?;
    sqlx::query("DELETE FROM sessions WHERE principal_id = $1 AND token_hash != $2")
        .bind(id)
        .bind(keep)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
/// A new password is a new device of the pass account: one transaction adds
/// it and removes the old one. What the new device is stored as is written
/// first, so that a lost answer cannot leave the account without its record.
async fn change_device(
    app: &App,
    chain: &chain_provider::Provider,
    session: &Session,
    current_password: String,
    new_password: String,
) -> Result<StatusCode, Error> {
    let id = session.view.principal_id.to_string();
    let row = sqlx::query(
        "SELECT username, password_hash, password_hash_next FROM principals WHERE principal_id = $1",
    )
    .bind(&id)
    .fetch_one(&app.db)
    .await?;
    let name: String = row.try_get("username")?;
    // An earlier change whose answer was lost is settled first.
    let stored = settle(
        app,
        chain,
        &id,
        &name,
        row.try_get("password_hash")?,
        row.try_get("password_hash_next")?,
        Some(&session.token_hash),
    )
    .await?;
    // Every key is derived before anything is written.
    let Some(old) = unlock_device(app, Zeroizing::new(current_password), &stored).await? else {
        return Err(Error::Unauthorized);
    };
    let (next, new) = new_device(app, Zeroizing::new(new_password)).await?;
    // From the claim to the promotion the change is one piece of work. It
    // runs to its end whoever waits, and holds the lock `settle` takes: the
    // chain is not asked about a change that is still on its way.
    let (db, chain, lock) = (app.db.clone(), chain.clone(), app.credentials.clone());
    let (principal, keep) = (id.clone(), session.token_hash.to_vec());
    tokio::spawn(async move {
        let _held = lock.lock_owned().await;
        let changed = replace_device(
            &db, &chain, &principal, &name, &stored, &next, &old, &new, keep,
        )
        .await;
        if let Err(error) = &changed {
            tracing::warn!(?error, "a password change did not complete");
        }
        changed
    })
    .await??;
    app.audit(Some(&id), None, "password_changed", "ok").await?;
    Ok(StatusCode::NO_CONTENT)
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
    if let Some(chain) = &app.chain {
        return change_device(app, chain, session, current_password, new_password).await;
    }
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
