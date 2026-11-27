//! The passkey ceremonies when a passkey is a device of a pass account.
//!
//! The routes and their bodies are the same. What differs is whose challenge
//! the passkey answers and who verifies the answer: the chain's and the
//! chain. The API checks only what the chain does not, before submitting.
use super::{CEREMONY_SECONDS, CeremonyOptions, LoginVerifyRequest, RegisterVerifyRequest};
use crate::auth::{Session, chain_error, create_session, settle, unlock_device};
use crate::state::{App, Error, now, parse, token};
use axum::{http::StatusCode, response::Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chain_provider::passkeys::{Asserted, Created, RelyingParty, assertion, registration};
use chain_provider::{Enrollment, Key, PasskeyLogin, Provider, passkey_device};
use protocol::{PrincipalId, WalletLifecycle, WalletView};
use serde_json::json;
use sqlx::Row;
use std::env;
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse};
use zeroize::Zeroizing;

/// Most ceremonies under way at once; a new one beyond it is refused.
const MAX_CEREMONIES: usize = 4096;
/// Most logins of one principal under way at once; the oldest gives way to
/// a new one. Anyone can ask for a login, so one must not end another's.
const MAX_LOGINS: usize = 8;

/// A ceremony between its two requests. It lives in memory only: a
/// registration holds the password's device key for the five minutes the
/// second request may take, and a login holds the session's key.
pub(crate) enum Ceremony {
    Register {
        principal: PrincipalId,
        session: [u8; 32],
        admin: Key,
        enrollment: Enrollment,
        expires: i64,
    },
    Login {
        principal: PrincipalId,
        login: PasskeyLogin,
        expires: i64,
    },
}

impl Ceremony {
    const fn principal(&self) -> PrincipalId {
        match self {
            Self::Register { principal, .. } | Self::Login { principal, .. } => *principal,
        }
    }

    const fn expires(&self) -> i64 {
        match self {
            Self::Register { expires, .. } | Self::Login { expires, .. } => *expires,
        }
    }
}

fn relying_party() -> RelyingParty {
    let origin = env::var("WEBAUTHN_ORIGIN").unwrap_or_else(|_| "http://localhost:8088".to_owned());
    RelyingParty {
        id: env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| "localhost".to_owned()),
        origins: vec![origin],
    }
}

/// The relying party must be the host of the page: the chain accepts a
/// passkey only for the host of the page that created it.
pub(crate) fn check_relying_party() -> Result<(), Error> {
    let party = relying_party();
    let host = party
        .origins
        .first()
        .and_then(|origin| reqwest::Url::parse(origin).ok())
        .and_then(|origin| origin.host_str().map(str::to_owned));
    if host.as_deref() == Some(party.id.as_str()) {
        Ok(())
    } else {
        tracing::error!("WEBAUTHN_RP_ID must be the host of WEBAUTHN_ORIGIN");
        Err(Error::Config)
    }
}

/// Forget the ceremonies whose time is over, and with them the keys they hold.
pub(crate) fn purge(app: &App) {
    if let (Ok(time), Ok(mut ceremonies)) = (now(), app.ceremonies.lock()) {
        ceremonies.retain(|_, ceremony| ceremony.expires() > time);
    }
}

fn begin(app: &App, ceremony: Ceremony) -> Result<String, Error> {
    let id = token()?.to_string();
    let time = now()?;
    let mut ceremonies = app.ceremonies.lock().map_err(|_| Error::Internal)?;
    ceremonies.retain(|_, other| other.expires() > time);
    if ceremonies.len() >= MAX_CEREMONIES {
        return Err(Error::Capacity);
    }
    // One registration per principal, and a few logins: the oldest give way.
    let allowed = match ceremony {
        Ceremony::Register { .. } => 1,
        Ceremony::Login { .. } => MAX_LOGINS,
    };
    let mut alike: Vec<(i64, String)> = ceremonies
        .iter()
        .filter(|(_, other)| {
            other.principal() == ceremony.principal()
                && std::mem::discriminant(*other) == std::mem::discriminant(&ceremony)
        })
        .map(|(id, other)| (other.expires(), id.clone()))
        .collect();
    alike.sort();
    let excess = (alike.len() + 1).saturating_sub(allowed);
    for (_, oldest) in alike.into_iter().take(excess) {
        ceremonies.remove(&oldest);
    }
    ceremonies.insert(id.clone(), ceremony);
    Ok(id)
}

/// Take a ceremony: it is used once.
fn take(app: &App, id: &str) -> Result<Ceremony, Error> {
    let ceremony = app
        .ceremonies
        .lock()
        .map_err(|_| Error::Internal)?
        .remove(id)
        .ok_or(Error::Unauthorized)?;
    if ceremony.expires() <= now()? {
        return Err(Error::Unauthorized);
    }
    Ok(ceremony)
}

/// The password's device of the session's principal, and their username.
async fn admin_device(
    app: &App,
    chain: &Provider,
    session: &Session,
    password: String,
) -> Result<(Key, String, String), Error> {
    let id = session.view.principal_id.to_string();
    let row = sqlx::query(
        "SELECT username, display_name, password_hash, password_hash_next FROM principals WHERE principal_id = $1",
    )
    .bind(&id)
    .fetch_one(&app.db)
    .await?;
    let username: String = row.try_get("username")?;
    // A password change whose answer was lost is settled first.
    let stored = settle(
        app,
        chain,
        &id,
        &username,
        row.try_get("password_hash")?,
        row.try_get("password_hash_next")?,
        Some(&session.token_hash),
    )
    .await?;
    let device = unlock_device(app, Zeroizing::new(password), &stored)
        .await?
        .ok_or(Error::Unauthorized)?;
    Ok((device, username, row.try_get("display_name")?))
}

/// Bring the rows of `principal`'s passkeys whose registration was cut short
/// in line with the chain: a passkey the chain has is confirmed, and one it
/// does not have is forgotten. A registration that may still be on its way
/// is left alone, and so is a row the chain cannot be asked about.
pub(super) async fn reconcile(
    app: &App,
    chain: &Provider,
    principal: PrincipalId,
) -> Result<(), Error> {
    let rows = sqlx::query(
        "SELECT c.credential_ref, c.credential_id, p.username FROM passkey_credentials c JOIN principals p USING (principal_id) WHERE c.principal_id = $1 AND c.passkey_json::jsonb ? 'pending' AND c.created_at < $2",
    )
    .bind(principal.to_string())
    .bind(now()? - CEREMONY_SECONDS)
    .fetch_all(&app.db)
    .await?;
    for row in rows {
        let (reference, id): (String, Vec<u8>) = (
            row.try_get("credential_ref")?,
            row.try_get("credential_id")?,
        );
        let username: String = row.try_get("username")?;
        match chain.has_device(&username, passkey_device(&id)).await {
            Ok(true) => {
                sqlx::query("UPDATE passkey_credentials SET passkey_json = $2 WHERE credential_ref = $1 AND passkey_json::jsonb ? 'pending'")
                    .bind(&reference)
                    .bind(confirmed(&id))
                    .execute(&app.db)
                    .await?;
            }
            Ok(false) => forget(&app.db, &reference).await?,
            Err(error) => tracing::warn!(%error, "a passkey's registration is not settled"),
        }
    }
    Ok(())
}

/// What the row of a passkey the chain has holds.
fn confirmed(credential_id: &[u8]) -> String {
    json!({"device": passkey_device(credential_id).to_string()}).to_string()
}

/// Forget a passkey whose registration did not reach the chain.
async fn forget(db: &sqlx::PgPool, reference: &str) -> Result<(), Error> {
    sqlx::query(
        "DELETE FROM passkey_credentials WHERE credential_ref = $1 AND passkey_json::jsonb ? 'pending'",
    )
    .bind(reference)
    .execute(db)
    .await?;
    Ok(())
}

/// A passkey is registered once: a second row for it is a conflict.
fn registered_once(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        Error::Conflict("credential_already_registered")
    } else {
        error.into()
    }
}

/// Register a passkey: its row, the device on the chain, and the row's
/// confirmation. The row comes first, so that a passkey the chain took is
/// never one the account cannot see or remove; until the chain confirms it,
/// it is listed but does not log in.
async fn register(
    db: sqlx::PgPool,
    chain: Provider,
    principal: PrincipalId,
    admin: Key,
    enrollment: Enrollment,
    passkey: chain_provider::PasskeyRegistration,
) -> Result<(), Error> {
    let reference = token()?.to_string();
    let credential_id = passkey.credential_id.clone();
    let username: String =
        sqlx::query_scalar("SELECT username FROM principals WHERE principal_id = $1")
            .bind(principal.to_string())
            .fetch_one(&db)
            .await?;
    sqlx::query("INSERT INTO passkey_credentials(credential_ref, credential_id, principal_id, passkey_json, created_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(&reference)
        .bind(&credential_id)
        .bind(principal.to_string())
        .bind(json!({"pending": true}).to_string())
        .bind(now()?)
        .execute(&db)
        .await
        .map_err(registered_once)?;
    let added = chain
        .add_passkey(&admin, enrollment, passkey)
        .await
        .map(|_| ())
        .map_err(chain_error);
    let on_chain = match &added {
        Ok(()) => true,
        // The chain refused the password's device: nothing was added.
        Err(Error::Unauthorized) => false,
        // Whether the passkey was added is not known: the chain says. If it
        // cannot be asked, the row waits for `reconcile`.
        Err(_) => match chain
            .has_device(&username, passkey_device(&credential_id))
            .await
        {
            Ok(on_chain) => on_chain,
            Err(_) => return added,
        },
    };
    if !on_chain {
        forget(&db, &reference).await?;
        return added;
    }
    // The row may have been removed while the chain was adding the device.
    sqlx::query("INSERT INTO passkey_credentials(credential_ref, credential_id, principal_id, passkey_json, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (credential_ref) DO UPDATE SET passkey_json = EXCLUDED.passkey_json")
        .bind(&reference)
        .bind(&credential_id)
        .bind(principal.to_string())
        .bind(confirmed(&credential_id))
        .bind(now()?)
        .execute(&db)
        .await
        .map_err(registered_once)?;
    Ok(())
}

/// The passkeys the chain is known to have for `principal`, newest first.
async fn credential_ids(app: &App, principal: PrincipalId) -> Result<Vec<Vec<u8>>, Error> {
    let rows = sqlx::query(
        "SELECT credential_id FROM passkey_credentials WHERE principal_id = $1 AND passkey_json::jsonb ? 'device' ORDER BY created_at DESC, credential_ref",
    )
    .bind(principal.to_string())
    .fetch_all(&app.db)
    .await?;
    rows.into_iter()
        .map(|row| row.try_get("credential_id").map_err(Error::from))
        .collect()
}

pub(super) async fn register_options(
    app: &App,
    chain: &Provider,
    session: &Session,
    password: String,
) -> Result<CeremonyOptions<CreationChallengeResponse>, Error> {
    let (admin, username, display_name) = admin_device(app, chain, session, password).await?;
    let principal = session.view.principal_id;
    reconcile(app, chain, principal).await?;
    let enrollment = chain
        .passkey_enrollment(&username)
        .await
        .map_err(chain_error)?;
    let party = relying_party();
    let excluded: Vec<_> = credential_ids(app, principal)
        .await?
        .iter()
        .map(|id| json!({"type": "public-key", "id": URL_SAFE_NO_PAD.encode(id)}))
        .collect();
    // The chain takes P-256 keys only, and the user must be verified.
    let options = serde_json::from_value(json!({"publicKey": {
        "rp": {"id": party.id, "name": party.id},
        "user": {
            "id": URL_SAFE_NO_PAD.encode(principal.into_bytes()),
            "name": username,
            "displayName": display_name,
        },
        "challenge": URL_SAFE_NO_PAD.encode(enrollment.challenge),
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
        "timeout": CEREMONY_SECONDS * 1000,
        "attestation": "none",
        "excludeCredentials": excluded,
        "authenticatorSelection": {"userVerification": "required", "requireResidentKey": false},
    }}))?;
    let ceremony_id = begin(
        app,
        Ceremony::Register {
            principal,
            session: session.token_hash,
            admin,
            enrollment,
            expires: now()? + CEREMONY_SECONDS,
        },
    )?;
    Ok(CeremonyOptions {
        ceremony_id,
        options,
    })
}

pub(super) async fn register_verify(
    app: &App,
    chain: &Provider,
    session: &Session,
    request: RegisterVerifyRequest,
) -> Result<StatusCode, Error> {
    let Ceremony::Register {
        principal,
        session: owner,
        admin,
        enrollment,
        ..
    } = take(app, &request.ceremony_id)?
    else {
        return Err(Error::Unauthorized);
    };
    if principal != session.view.principal_id || owner != session.token_hash {
        return Err(Error::Unauthorized);
    }
    let created = Created {
        attestation_object: request.credential.response.attestation_object.to_vec(),
        client_data_json: request.credential.response.client_data_json.to_vec(),
    };
    let passkey =
        registration(&created, &enrollment.challenge, &relying_party()).map_err(chain_error)?;
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM passkey_credentials WHERE credential_id = $1)",
    )
    .bind(&passkey.credential_id)
    .fetch_one(&app.db)
    .await?;
    if known {
        return Err(Error::Conflict("credential_already_registered"));
    }
    // The registration runs to its end whoever waits for it.
    let work = register(
        app.db.clone(),
        chain.clone(),
        principal,
        admin,
        enrollment,
        passkey,
    );
    tokio::spawn(async move {
        let registered = work.await;
        if let Err(error) = &registered {
            tracing::warn!(?error, "a passkey registration did not complete");
        }
        registered
    })
    .await??;
    app.audit(
        Some(&principal.to_string()),
        None,
        "passkey_registered",
        "ok",
    )
    .await?;
    Ok(StatusCode::CREATED)
}

pub(super) async fn remove(
    app: &App,
    chain: &Provider,
    session: &Session,
    credential_ref: String,
    password: String,
) -> Result<StatusCode, Error> {
    let (admin, username, _) = admin_device(app, chain, session, password).await?;
    let principal = session.view.principal_id.to_string();
    let credential_id: Vec<u8> = sqlx::query_scalar(
        "SELECT credential_id FROM passkey_credentials WHERE credential_ref = $1 AND principal_id = $2",
    )
    .bind(&credential_ref)
    .bind(&principal)
    .fetch_optional(&app.db)
    .await?
    .ok_or(Error::NotFound)?;
    chain
        .remove_device(&username, &admin, passkey_device(&credential_id))
        .await
        .map_err(chain_error)?;
    sqlx::query("DELETE FROM passkey_credentials WHERE credential_ref = $1 AND principal_id = $2")
        .bind(credential_ref)
        .bind(&principal)
        .execute(&app.db)
        .await?;
    app.audit(Some(&principal), None, "passkey_removed", "ok")
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn login_options(
    app: &App,
    chain: &Provider,
    principal: PrincipalId,
    username: &str,
) -> Result<CeremonyOptions<RequestChallengeResponse>, Error> {
    // The transaction that opens the session names its device, so a login is
    // for one passkey: the one registered last.
    let credential_id = credential_ids(app, principal)
        .await?
        .into_iter()
        .next()
        .ok_or(Error::Unauthorized)?;
    let login = chain
        .passkey_login(username, passkey_device(&credential_id))
        .await
        .map_err(chain_error)?;
    let options = serde_json::from_value(json!({"publicKey": {
        "challenge": URL_SAFE_NO_PAD.encode(login.challenge()),
        "timeout": CEREMONY_SECONDS * 1000,
        "rpId": relying_party().id,
        "allowCredentials": [{"type": "public-key", "id": URL_SAFE_NO_PAD.encode(&credential_id)}],
        "userVerification": "required",
    }}))?;
    let ceremony_id = begin(
        app,
        Ceremony::Login {
            principal,
            login,
            expires: now()? + CEREMONY_SECONDS,
        },
    )?;
    Ok(CeremonyOptions {
        ceremony_id,
        options,
    })
}

pub(super) async fn login_verify(
    app: &App,
    chain: &Provider,
    request: LoginVerifyRequest,
) -> Result<Response, Error> {
    let Ceremony::Login {
        principal, login, ..
    } = take(app, &request.ceremony_id)?
    else {
        return Err(Error::Unauthorized);
    };
    let asserted = Asserted {
        authenticator_data: request.credential.response.authenticator_data.to_vec(),
        client_data_json: request.credential.response.client_data_json.to_vec(),
        signature: request.credential.response.signature.to_vec(),
    };
    let answer = assertion(&asserted, &login.challenge(), &relying_party()).map_err(chain_error)?;
    let (account, _) = chain
        .open_passkey_session(login, answer)
        .await
        .map_err(chain_error)?;
    let wallet: String =
        sqlx::query_scalar("SELECT wallet_id FROM principals WHERE principal_id = $1")
            .bind(principal.to_string())
            .fetch_one(&app.db)
            .await?;
    let wallet = WalletView {
        wallet_id: parse(&wallet)?,
        principal_id: Some(principal),
        account_id: account,
        lifecycle: WalletLifecycle::Active,
    };
    app.audit(Some(&principal.to_string()), None, "passkey_login", "ok")
        .await?;
    create_session(app, principal, wallet, StatusCode::OK).await
}
