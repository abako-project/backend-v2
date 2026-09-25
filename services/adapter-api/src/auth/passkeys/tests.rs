use super::*;
use sqlx::PgPool;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn server() -> Result<Webauthn, Box<dyn std::error::Error>> {
    let origin = Url::parse("http://localhost:8088")?;
    Ok(WebauthnBuilder::new("localhost", &origin)?.build()?)
}

#[test]
fn virtual_authenticator_verifies_and_rejects_foreign_origin_or_challenge() -> TestResult {
    let server = server()?;
    let origin = Url::parse("http://localhost:8088")?;
    let foreign = Url::parse("http://localhost:8089")?;
    // The test-only soft passkey must simulate UV; it cannot perform real PIN/biometrics.
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let (options, registration) =
        server.start_passkey_registration(Uuid::new_v4(), "alice", "Alice", None)?;
    let proof = authenticator.do_registration(origin.clone(), options)?;
    let passkey = server.finish_passkey_registration(&proof, &registration)?;

    let (options, state) = server.start_passkey_authentication(std::slice::from_ref(&passkey))?;
    let assertion = authenticator.do_authentication(origin, options)?;
    let result = server.finish_passkey_authentication(&assertion, &state)?;
    assert!(result.user_verified());
    assert_eq!(result.cred_id(), passkey.cred_id());

    let (options, other_state) =
        server.start_passkey_authentication(std::slice::from_ref(&passkey))?;
    let other_assertion = authenticator.do_authentication(foreign, options)?;
    assert!(
        server
            .finish_passkey_authentication(&other_assertion, &other_state)
            .is_err()
    );
    assert!(
        server
            .finish_passkey_authentication(&assertion, &other_state)
            .is_err()
    );
    Ok(())
}

#[test]
fn virtual_authenticator_without_user_verification_cannot_register() -> TestResult {
    let server = server()?;
    let origin = Url::parse("http://localhost:8088")?;
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(false));
    let (options, registration) =
        server.start_passkey_registration(Uuid::new_v4(), "alice", "Alice", None)?;
    assert!(authenticator.do_registration(origin, options).is_err());
    drop(registration);
    Ok(())
}

async fn database() -> Result<PgPool, Box<dyn std::error::Error>> {
    let base = std::env::var("TEST_ADAPTER_DATABASE_URL")?;
    let schema = format!(
        "passkey_test_{:032x}",
        u128::from_le_bytes(crate::state::random()?)
    );
    let admin = PgPool::connect(&base).await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    admin.close().await;
    let separator = if base.contains('?') { '&' } else { '?' };
    let db = PgPool::connect(&format!(
        "{base}{separator}options=-csearch_path%3D{schema}"
    ))
    .await?;
    sqlx::raw_sql(include_str!("../../../migrations/0001_adapter.sql"))
        .execute(&db)
        .await?;
    sqlx::raw_sql(include_str!("../../../migrations/0003_passkeys.sql"))
        .execute(&db)
        .await?;
    Ok(db)
}

#[tokio::test]
async fn ceremony_is_session_bound_single_use_and_expires() -> TestResult {
    let db = database().await?;
    let alice = PrincipalId::from_bytes([1; 16]);
    let bob = PrincipalId::from_bytes([2; 16]);
    for (id, name) in [(alice, "alice"), (bob, "bob")] {
        sqlx::query("INSERT INTO principals(principal_id, username, password_hash, display_name, is_admin, created_at) VALUES ($1, $2, 'test-not-a-hash', $3, FALSE, 0)")
            .bind(id.to_string()).bind(name).bind(name).execute(&db).await?;
    }
    let server = server()?;
    let (_, registration) =
        server.start_passkey_registration(Uuid::new_v4(), "alice", "Alice", None)?;
    let ceremony = token()?;
    let session = [3; 32];
    let other_session = [4; 32];
    store::insert_ceremony(
        &db,
        &ceremony,
        alice,
        "register",
        &registration,
        Some(&session),
        now()? + CEREMONY_SECONDS,
    )
    .await?;
    assert!(
        store::consume_ceremony(&db, &ceremony, "register", Some(bob), Some(&session))
            .await
            .is_err()
    );
    assert!(
        store::consume_ceremony(
            &db,
            &ceremony,
            "register",
            Some(alice),
            Some(&other_session)
        )
        .await
        .is_err()
    );
    let saved =
        store::consume_ceremony(&db, &ceremony, "register", Some(alice), Some(&session)).await?;
    let _: webauthn_rs::prelude::PasskeyRegistration = serde_json::from_str(&saved)?;
    assert!(
        store::consume_ceremony(&db, &ceremony, "register", Some(alice), Some(&session))
            .await
            .is_err()
    );

    let expired = token()?;
    store::insert_ceremony(
        &db,
        &expired,
        alice,
        "register",
        &registration,
        Some(&session),
        now()? - 1,
    )
    .await?;
    assert!(
        store::consume_ceremony(&db, &expired, "register", Some(alice), Some(&session))
            .await
            .is_err()
    );
    db.close().await;
    Ok(())
}
