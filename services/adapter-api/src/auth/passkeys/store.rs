use crate::state::{Error, now, parse, token};
use generated_contracts::PrincipalId;
use serde::Serialize;
use sqlx::{PgPool, Row};
use webauthn_rs::prelude::{AuthenticationResult, Passkey};

pub(super) async fn credentials(
    db: &PgPool,
    principal: PrincipalId,
) -> Result<Vec<Passkey>, Error> {
    let rows = sqlx::query(
        "SELECT passkey_json FROM passkey_credentials WHERE principal_id = $1 ORDER BY created_at",
    )
    .bind(principal.to_string())
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| serde_json::from_str(row.try_get("passkey_json")?).map_err(Error::from))
        .collect()
}

pub(super) async fn insert_ceremony<T: Serialize>(
    db: &PgPool,
    ceremony_id: &str,
    principal: PrincipalId,
    kind: &str,
    state: &T,
    session_hash: Option<&[u8; 32]>,
    expires_at: i64,
) -> Result<(), Error> {
    sqlx::query("DELETE FROM passkey_ceremonies WHERE expires_at <= $1")
        .bind(now()?)
        .execute(db)
        .await?;
    sqlx::query("INSERT INTO passkey_ceremonies(ceremony_id, principal_id, kind, state_json, session_token_hash, expires_at) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(ceremony_id)
        .bind(principal.to_string())
        .bind(kind)
        .bind(serde_json::to_string(state)?)
        .bind(session_hash.map(|value| value.to_vec()))
        .bind(expires_at)
        .execute(db)
        .await?;
    Ok(())
}

async fn consume(
    db: &PgPool,
    ceremony_id: &str,
    kind: &str,
    principal: Option<PrincipalId>,
    session_hash: Option<&[u8; 32]>,
) -> Result<(PrincipalId, String), Error> {
    if ceremony_id.len() != 66 {
        return Err(Error::Unauthorized);
    }
    let row = sqlx::query("DELETE FROM passkey_ceremonies WHERE ceremony_id = $1 AND kind = $2 AND expires_at > $3 AND ($4::TEXT IS NULL OR principal_id = $4) AND ($5::BYTEA IS NULL OR session_token_hash = $5) RETURNING principal_id, state_json")
        .bind(ceremony_id)
        .bind(kind)
        .bind(now()?)
        .bind(principal.map(|id| id.to_string()))
        .bind(session_hash.map(|value| value.to_vec()))
        .fetch_optional(db)
        .await?
        .ok_or(Error::Unauthorized)?;
    Ok((
        parse(row.try_get("principal_id")?)?,
        row.try_get("state_json")?,
    ))
}

pub(super) async fn consume_ceremony(
    db: &PgPool,
    ceremony_id: &str,
    kind: &str,
    principal: Option<PrincipalId>,
    session_hash: Option<&[u8; 32]>,
) -> Result<String, Error> {
    consume(db, ceremony_id, kind, principal, session_hash)
        .await
        .map(|(_, state)| state)
}

pub(super) async fn consume_login(
    db: &PgPool,
    ceremony_id: &str,
) -> Result<(PrincipalId, String), Error> {
    consume(db, ceremony_id, "login", None, None).await
}

pub(super) async fn insert_credential(
    db: &PgPool,
    principal: PrincipalId,
    passkey: &Passkey,
) -> Result<(), Error> {
    let affected = sqlx::query("INSERT INTO passkey_credentials(credential_ref, credential_id, principal_id, passkey_json, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT(credential_id) DO NOTHING")
        .bind(token()?.as_str())
        .bind(passkey.cred_id().as_slice())
        .bind(principal.to_string())
        .bind(serde_json::to_string(passkey)?)
        .bind(now()?)
        .execute(db)
        .await?;
    if affected.rows_affected() != 1 {
        return Err(Error::Conflict("credential_already_registered"));
    }
    Ok(())
}

pub(super) async fn update_credential(
    db: &PgPool,
    principal: PrincipalId,
    result: &AuthenticationResult,
) -> Result<(), Error> {
    let mut transaction = db.begin().await?;
    let row = sqlx::query("SELECT passkey_json, sign_count FROM passkey_credentials WHERE credential_id = $1 AND principal_id = $2 FOR UPDATE")
        .bind(result.cred_id().as_slice())
        .bind(principal.to_string())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::Unauthorized)?;
    let mut passkey: Passkey = serde_json::from_str(row.try_get("passkey_json")?)?;
    let stored: i64 = row.try_get("sign_count")?;
    let observed = i64::from(result.counter());
    if (stored > 0 || observed > 0) && observed <= stored {
        return Err(Error::Unauthorized);
    }
    if passkey.update_credential(result).is_none() {
        return Err(Error::Unauthorized);
    }
    sqlx::query("UPDATE passkey_credentials SET passkey_json = $1, sign_count = $2 WHERE credential_id = $3 AND principal_id = $4")
        .bind(serde_json::to_string(&passkey)?)
        .bind(observed)
        .bind(result.cred_id().as_slice())
        .bind(principal.to_string())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
