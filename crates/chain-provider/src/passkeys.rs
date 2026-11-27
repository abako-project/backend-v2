//! What a browser returns for a passkey, checked and turned into what the
//! chain takes.
//!
//! The chain verifies a passkey's signature and that it answers the chain's
//! own challenge. On a login it does not check where the answer was made or
//! that the user was verified, so those checks are made here, before
//! anything is submitted.
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kreivo::{PasskeyAssertion, PasskeyRegistration};
use serde::Deserialize;
use serde_cbor_2::Value;
use sha2::{Digest, Sha256};

use crate::{Error, transport};

/// The start of a DER `SubjectPublicKeyInfo` for a P-256 key: the algorithm
/// (id-ecPublicKey, prime256v1) and the header of the 65-byte point.
const P256_SPKI: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];
/// Authenticator data flags: user present, user verified, attested credential.
const USER_PRESENT: u8 = 0x01;
const USER_VERIFIED: u8 = 0x04;
const ATTESTED: u8 = 0x40;

/// Who the API is to a passkey. The chain accepts a passkey only for the host
/// of the page that created it, so `id` is that host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelyingParty {
    /// The relying party ID: the host of the pages below.
    pub id: String,
    /// The origins whose pages may use the passkey.
    pub origins: Vec<String>,
}

/// A passkey's answer to WebAuthn's `create()`.
#[derive(Clone, Debug)]
pub struct Created {
    /// The attestation object, CBOR.
    pub attestation_object: Vec<u8>,
    /// The client data JSON.
    pub client_data_json: Vec<u8>,
}

/// A passkey's answer to WebAuthn's `get()`.
#[derive(Clone, Debug)]
pub struct Asserted {
    /// The authenticator data.
    pub authenticator_data: Vec<u8>,
    /// The client data JSON.
    pub client_data_json: Vec<u8>,
    /// The signature.
    pub signature: Vec<u8>,
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: bool,
}

/// The most client data the chain takes.
const MAX_CLIENT_DATA: usize = 1024;

fn refused() -> Error {
    Error::Provider(transport::Error::domain("invalid_credentials"))
}

fn require(condition: bool) -> Result<(), Error> {
    if condition { Ok(()) } else { Err(refused()) }
}

/// The client data is of this kind, for this challenge, from an allowed page.
fn client_data(
    json: &[u8],
    kind: &str,
    challenge: &[u8; 32],
    party: &RelyingParty,
) -> Result<(), Error> {
    require(json.len() <= MAX_CLIENT_DATA)?;
    let data: ClientData = serde_json::from_slice(json).map_err(|_| refused())?;
    require(!data.cross_origin)?;
    // An origin has no path; some clients still write its root.
    let origin = data.origin.strip_suffix('/').unwrap_or(&data.origin);
    require(
        data.kind == kind
            && data.challenge == URL_SAFE_NO_PAD.encode(challenge)
            && party.origins.iter().any(|allowed| allowed == origin),
    )
}

/// The authenticator data is for this relying party, with the user present
/// and verified. Returns its flags.
fn authenticator_data(data: &[u8], party: &RelyingParty) -> Result<u8, Error> {
    let (Some(hash), Some(flags)) = (data.get(..32), data.get(32)) else {
        return Err(refused());
    };
    require(
        data.len() >= 37
            && hash == Sha256::digest(party.id.as_bytes()).as_slice()
            && flags & USER_PRESENT != 0
            && flags & USER_VERIFIED != 0,
    )?;
    Ok(*flags)
}

/// The credential ID and DER public key attested in `data`.
fn attested_credential(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    // rpIdHash (32) | flags (1) | signCount (4) | aaguid (16) | len (2) | id | key
    let rest = data.get(53..).ok_or_else(refused)?;
    let (length, rest) = rest.split_first_chunk::<2>().ok_or_else(refused)?;
    let length = usize::from(u16::from_be_bytes(*length));
    require(rest.len() > length)?;
    let (credential_id, cose) = rest.split_at(length);
    // The COSE key may be followed by extension data.
    let key = serde_cbor_2::Deserializer::from_slice(cose)
        .into_iter::<Value>()
        .next()
        .ok_or_else(refused)?
        .map_err(|_| refused())?;
    let Value::Map(key) = key else {
        return Err(refused());
    };
    let integer = |label: i128| match key.get(&Value::Integer(label)) {
        Some(Value::Integer(value)) => Some(*value),
        _ => None,
    };
    let coordinate = |label: i128| match key.get(&Value::Integer(label)) {
        Some(Value::Bytes(bytes)) if bytes.len() == 32 => Some(bytes.as_slice()),
        _ => None,
    };
    // kty EC2 (2), alg ES256 (-7), crv P-256 (1): the only key the chain takes.
    require(integer(1) == Some(2) && integer(3) == Some(-7) && integer(-1) == Some(1))?;
    let (Some(x), Some(y)) = (coordinate(-2), coordinate(-3)) else {
        return Err(refused());
    };
    let public_key = [&P256_SPKI[..], &[0x04], x, y].concat();
    Ok((credential_id.to_vec(), public_key))
}

/// Check a new passkey's answer to `challenge` and take from it what the
/// chain registers.
///
/// # Errors
/// `invalid_credentials` if the answer is not a P-256 passkey created for
/// this challenge, on an allowed page, with the user verified.
pub fn registration(
    created: &Created,
    challenge: &[u8; 32],
    party: &RelyingParty,
) -> Result<PasskeyRegistration, Error> {
    client_data(
        &created.client_data_json,
        "webauthn.create",
        challenge,
        party,
    )?;
    let object: Value =
        serde_cbor_2::from_slice(&created.attestation_object).map_err(|_| refused())?;
    let Value::Map(object) = object else {
        return Err(refused());
    };
    let Some(Value::Bytes(data)) = object.get(&Value::Text("authData".into())) else {
        return Err(refused());
    };
    require(authenticator_data(data, party)? & ATTESTED != 0)?;
    let (credential_id, public_key) = attested_credential(data)?;
    Ok(PasskeyRegistration {
        credential_id,
        authenticator_data: data.clone(),
        client_data: created.client_data_json.clone(),
        public_key,
    })
}

/// Check a passkey's answer to `challenge` and take from it what the chain
/// verifies.
///
/// # Errors
/// `invalid_credentials` if the answer was not made for this challenge, on an
/// allowed page, with the user verified.
pub fn assertion(
    asserted: &Asserted,
    challenge: &[u8; 32],
    party: &RelyingParty,
) -> Result<PasskeyAssertion, Error> {
    client_data(&asserted.client_data_json, "webauthn.get", challenge, party)?;
    authenticator_data(&asserted.authenticator_data, party)?;
    Ok(PasskeyAssertion {
        authenticator_data: asserted.authenticator_data.clone(),
        client_data: asserted.client_data_json.clone(),
        signature: asserted.signature.clone(),
    })
}
