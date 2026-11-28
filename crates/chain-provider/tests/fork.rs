//! The provider's interface on a fork of Kreivo. Needs a snapshot:
//! `KREIVO_FORK_SNAPSHOT=<path> cargo test -p chain-provider --features fork --release -- --include-ignored`.
#![cfg(feature = "fork")]

use std::error::Error;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chain_provider::passkeys::{self, Asserted, Created, RelyingParty};
use chain_provider::transport::ProviderApi;
use chain_provider::{Config, Provider, passkey_device};
use kreivo::Key;
use protocol::provider_core::{MetadataCommitmentV1, RegisterWorkerCoreRequest};
use protocol::{
    AccountId32, CalendarDefinition, ExecutionOutcome, Minutes, OperationId, OperationReceipt,
    PayloadHash, PreparedCall, ProviderCommand, Qualifications, Sr25519Signature, UnixSeconds,
    UnsignedContractCallV1,
};
use serde_json::json;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs_proto::{CreationChallengeResponse, RequestChallengeResponse};
use zeroize::Zeroizing;

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

const CONTRACTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../services/contract-provider/fixtures"
);
const OPERATOR: &str = "operator@digesta";

fn key(seed: u8) -> Outcome<Key> {
    Ok(Key::from_seed(&Zeroizing::new([seed; 32]))?)
}

fn data_dir(name: &str) -> Outcome<PathBuf> {
    let directory =
        std::env::temp_dir().join(format!("chain-provider-{name}-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory)?;
    }
    Ok(directory)
}

async fn provider(data_dir: PathBuf) -> Outcome<Provider> {
    provider_in(data_dir, None).await
}

/// A provider whose accounts join this community, if there is one.
async fn provider_in(data_dir: PathBuf, community: Option<u16>) -> Outcome<Provider> {
    Ok(Provider::open(
        Config {
            snapshot: std::env::var("KREIVO_FORK_SNAPSHOT")?.into(),
            data_dir,
            contracts_dir: CONTRACTS.into(),
            mock_funding: true,
            operator_user: OPERATOR.into(),
            community,
        },
        Some(&key(1)?),
    )
    .await?)
}

fn registration_request(operation: u8) -> RegisterWorkerCoreRequest {
    RegisterWorkerCoreRequest {
        content: MetadataCommitmentV1 {
            operation_id: OperationId::from_bytes([operation; 16]),
            hash: PayloadHash::from_bytes([9; 32]),
        },
        qualifications: Qualifications {
            role_ids: vec![2],
            skill_ids: vec![1],
        },
        calendar: CalendarDefinition {
            default_weekly_minutes: Minutes::new(600),
            overrides: vec![],
        },
    }
}

async fn command(
    provider: &Provider,
    origin: AccountId32,
    operation: u8,
    command: ProviderCommand,
) -> Outcome<OperationReceipt> {
    let info = provider.info().await?;
    let nonce = provider.nonce(origin).await?.nonce;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let call = UnsignedContractCallV1::new(
        info.provider_instance_id,
        OperationId::from_bytes([operation; 16]),
        origin,
        nonce,
        UnixSeconds::new(now + 300),
        command,
    )?;
    let signed = PreparedCall::new(call)?.with_signature(Sr25519Signature::from_bytes([0; 64]));
    Ok(provider.execute(signed, UnixSeconds::new(now)).await?)
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_worker_registers_and_the_provider_resumes() -> Outcome {
    let directory = data_dir("register")?;
    let first = provider(directory.clone()).await?;
    let info = first.info().await?;
    assert_eq!(info.root_account, first.operator());

    let device = key(7)?;
    let alice = first.register("alice", &device).await?;
    assert_eq!(first.account("alice").await?, alice);
    // The same username cannot be registered again.
    assert!(first.register("alice", &key(8)?).await.is_err());

    // Without a session nothing is submitted for her.
    let request = |operation| ProviderCommand::RegisterWorkerCore(registration_request(operation));
    assert!(command(&first, alice, 1, request(1)).await.is_err());
    assert!(!first.has_session(alice)?);

    // Another device does not open her session; hers does.
    assert!(device_refused(first.open_session("alice", &key(8)?).await));
    let (account, until) = first.open_session("alice", &device).await?;
    assert_eq!(account, alice);
    assert!(first.has_session(alice)?);
    // A second login reuses the open session.
    assert_eq!(first.open_session("alice", &device).await?.1, until);

    let receipt = command(&first, alice, 1, request(1)).await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(receipt.origin, alice);
    assert_eq!(receipt.first_event_cursor, Some(1));
    // The same operation again returns the same receipt.
    assert_eq!(
        command_replay(&first, &receipt).await?,
        Some(receipt.clone())
    );
    // A second registration is the contract's refusal, kept as a failed receipt.
    let refused = command(&first, alice, 2, request(2)).await?;
    assert_eq!(
        refused.outcome,
        ExecutionOutcome::Failed("worker_exists".into())
    );
    assert_eq!(first.nonce(alice).await?.nonce, 2);
    let events = first.events(0, 10).await?;
    assert_eq!(events.events.len(), 1);
    assert_eq!(events.next_cursor, 1);

    // A new device replaces hers, as a new password does: it opens sessions
    // from then on, and the old one does not.
    let replacement = key(9)?;
    assert!(device_refused(
        first.replace_device("alice", &key(8)?, &replacement).await
    ));
    assert!(first.has_device("alice", device.account()).await?);
    first.replace_device("alice", &device, &replacement).await?;
    assert!(!first.has_device("alice", device.account()).await?);
    assert!(first.has_device("alice", replacement.account()).await?);
    assert!(device_refused(first.open_session("alice", &device).await));
    assert_eq!(first.open_session("alice", &replacement).await?.0, alice);

    // An account has ten sessions on the chain. Each new one ends the one
    // before it, so a dozen in a row, each by a new device, all open.
    let mut current = replacement;
    for seed in 20..32 {
        let next = key(seed)?;
        first.replace_device("alice", &current, &next).await?;
        first.open_session("alice", &next).await?;
        current = next;
    }

    // The reads see her: the snapshot, and the facts behind it.
    let snapshot = first.snapshot().await?;
    assert_eq!(snapshot.info, info);
    assert!(
        snapshot
            .workers
            .iter()
            .any(|worker| worker.account == alice)
    );
    assert!(!snapshot.catalog.roles.is_empty());
    first.task_state().await?;
    drop(first);

    // The saved fork and state come back; sessions do not.
    let again = provider(directory.clone()).await?;
    assert_eq!(again.info().await?, info);
    assert_eq!(again.receipt(receipt.operation_id).await?, Some(receipt));
    assert_eq!(again.nonce(alice).await?.nonce, 2);
    assert!(!again.has_session(alice)?);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

/// Whether the chain refused the device.
fn device_refused<T>(result: Result<T, chain_provider::Error>) -> bool {
    matches!(result, Err(chain_provider::Error::Provider(error)) if error.code() == "invalid_credentials")
}

async fn command_replay(
    provider: &Provider,
    receipt: &OperationReceipt,
) -> Outcome<Option<OperationReceipt>> {
    Ok(provider.receipt(receipt.operation_id).await?)
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_passkey_is_added_opens_sessions_and_is_removed() -> Outcome {
    let directory = data_dir("passkey")?;
    let provider = provider(directory.clone()).await?;
    let password = key(7)?;
    let alice = provider.register("alice", &password).await?;
    let party = RelyingParty {
        id: "localhost".into(),
        origins: vec!["http://localhost:8088".into()],
    };
    let origin = url::Url::parse("http://localhost:8088")?;
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // The passkey is created with the chain's challenge, and the password's
    // device adds it to the account.
    let enrollment = provider.passkey_enrollment("alice").await?;
    let options: CreationChallengeResponse = serde_json::from_value(json!({"publicKey": {
        "rp": {"id": "localhost", "name": "Digesta"},
        "user": {"id": URL_SAFE_NO_PAD.encode([7_u8; 16]), "name": "alice", "displayName": "Alice"},
        "challenge": URL_SAFE_NO_PAD.encode(enrollment.challenge),
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
        "timeout": 300_000,
        "attestation": "none",
        "authenticatorSelection": {"userVerification": "required", "requireResidentKey": false},
    }}))?;
    let created = authenticator
        .do_registration(origin.clone(), options)
        .map_err(|error| format!("create: {error:?}"))?;
    let created = Created {
        attestation_object: created.response.attestation_object.to_vec(),
        client_data_json: created.response.client_data_json.to_vec(),
    };
    // An answer to another challenge, or made on another page, is not taken.
    assert!(passkeys::registration(&created, &[0; 32], &party).is_err());
    let elsewhere = RelyingParty {
        id: "localhost".into(),
        origins: vec!["http://localhost:9999".into()],
    };
    assert!(passkeys::registration(&created, &enrollment.challenge, &elsewhere).is_err());
    let registration = passkeys::registration(&created, &enrollment.challenge, &party)
        .map_err(|error| format!("checking the new passkey: {error}"))?;
    let credential_id = registration.credential_id.clone();
    let device = provider
        .add_passkey(&password, enrollment, registration)
        .await
        .map_err(|error| format!("adding the passkey: {error}"))?;
    assert_eq!(device, passkey_device(&credential_id));

    // The passkey answers the challenge of a login, and its session signs.
    let login = provider.passkey_login("alice", device).await?;
    let challenge = login.challenge();
    let request = |challenge: [u8; 32]| -> Outcome<RequestChallengeResponse> {
        Ok(serde_json::from_value(json!({"publicKey": {
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "timeout": 300_000,
            "rpId": "localhost",
            "allowCredentials": [{"type": "public-key", "id": URL_SAFE_NO_PAD.encode(&credential_id)}],
            "userVerification": "required",
        }}))?)
    };
    let asserted = authenticator
        .do_authentication(origin.clone(), request(challenge)?)
        .map_err(|error| format!("get: {error:?}"))?;
    let asserted = Asserted {
        authenticator_data: asserted.response.authenticator_data.to_vec(),
        client_data_json: asserted.response.client_data_json.to_vec(),
        signature: asserted.response.signature.to_vec(),
    };
    let assertion = passkeys::assertion(&asserted, &challenge, &party)
        .map_err(|error| format!("checking the passkey's answer: {error}"))?;
    let (account, _) = provider
        .open_passkey_session(login, assertion)
        .await
        .map_err(|error| format!("opening the passkey's session: {error}"))?;
    assert_eq!(account, alice);
    let receipt = command(
        &provider,
        alice,
        1,
        ProviderCommand::RegisterWorkerCore(registration_request(1)),
    )
    .await?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);

    // Removed by the password's device, it opens nothing more.
    provider.remove_device("alice", &password, device).await?;
    let login = provider.passkey_login("alice", device).await?;
    let challenge = login.challenge();
    let asserted = authenticator
        .do_authentication(origin, request(challenge)?)
        .map_err(|error| format!("get: {error:?}"))?;
    let asserted = Asserted {
        authenticator_data: asserted.response.authenticator_data.to_vec(),
        client_data_json: asserted.response.client_data_json.to_vec(),
        signature: asserted.response.signature.to_vec(),
    };
    let assertion = passkeys::assertion(&asserted, &challenge, &party)?;
    assert!(
        provider
            .open_passkey_session(login, assertion)
            .await
            .is_err()
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn in_a_community_the_pool_pays_the_fees() -> Outcome {
    let calendar = || {
        ProviderCommand::SetCalendar(CalendarDefinition {
            default_weekly_minutes: Minutes::new(600),
            overrides: vec![],
        })
    };
    // What a command that stores nothing new costs its account, with and
    // without a community whose subscription pays.
    let mut costs = Vec::new();
    for community in [Some(4242), None] {
        let directory = data_dir(if community.is_some() { "pool" } else { "fees" })?;
        let provider = provider_in(directory.clone(), community).await?;
        let device = key(7)?;
        let alice = provider.register("alice", &device).await?;
        provider.open_session("alice", &device).await?;
        let registered = command(
            &provider,
            alice,
            1,
            ProviderCommand::RegisterWorkerCore(registration_request(1)),
        )
        .await?;
        assert_eq!(registered.outcome, ExecutionOutcome::Success);
        let before = provider.balance(alice).await?;
        let set = command(&provider, alice, 2, calendar()).await?;
        assert_eq!(set.outcome, ExecutionOutcome::Success);
        costs.push(before - provider.balance(alice).await?);
        // The community is set up at the first start only: a saved fork is
        // not resumed with another, or with none.
        drop(provider);
        let other = community.map_or(Some(4242), |_| None);
        let refused = provider_in(directory.clone(), other).await.err();
        assert!(
            refused
                .as_ref()
                .is_some_and(|error| error.to_string().contains("another community")),
            "{:?}",
            refused.map(|error| error.to_string())
        );
        std::fs::remove_dir_all(directory)?;
    }
    assert_eq!(costs[0], 0, "a member paid a fee");
    assert!(costs[1] > 0, "an account outside a community paid nothing");
    Ok(())
}
