//! pallet-pass accounts, devices and sessions.
//!
//! A device signs in a browser, so both ceremonies take two requests: the
//! first returns what to sign, the second submits the signature for the
//! chain to verify. The caller keeps the [`Enrollment`] or [`SessionRequest`]
//! in between, on the server, bound to whoever started it.

use core::cell::RefCell;

use blake2::{Blake2b, Digest, digest::consts::U32};
use parity_scale_codec::{Compact, Encode};
use pass::workflow::{ChallengeRequest, DeviceAttestation, DeviceAuthenticator};
use pass::{Account, DeviceFilter, DeviceId, HashedUserId, PassAuthenticator};
use protocol::AccountId32;
use sube::extrinsic::ChainContext;
use sube::{
    Backend, DynValue, ExtrinsicAssembler, Mortality, PreparedCall, TransactionOptions, WaitFor,
};

use crate::{Error, Key, Receipt, Result, worker::State};

/// The attestation and credential variant of key devices.
const SUBSTRATE_KEY: &str = "SubstrateKey";
/// The attestation and credential variant of passkeys.
const WEBAUTHN: &str = "WebAuthn";
/// How many blocks a request waiting for its device stays valid: inside the
/// 900 blocks Kreivo accepts a challenge for.
const REQUEST_BLOCKS: u64 = 512;
/// The only call a session key may make.
const SESSION_CALL: &str = "Contracts/call";

/// A pallet-pass user: the hash of their normalized username.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UserId([u8; 32]);

impl UserId {
    /// The user of an already normalized `username`.
    #[must_use]
    pub fn of(username: &str) -> Self {
        Self(Blake2b::<U32>::digest(username.as_bytes()).into())
    }

    /// The user with this hash.
    #[must_use]
    pub const fn from_bytes(hash: [u8; 32]) -> Self {
        Self(hash)
    }

    /// The hash.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// A registration waiting for the device's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
    /// The user registering.
    pub user: UserId,
    /// The pass account the registration creates.
    pub account: AccountId32,
    /// What a key device signs.
    pub message: Vec<u8>,
    /// What a passkey is created with: the challenge of its `create()`.
    pub challenge: [u8; 32],
    /// The block the challenge is bound to; the chain forgets old ones.
    pub block: crate::Block,
}

/// What a passkey answered when it was created, from `WebAuthn`'s `create()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasskeyRegistration {
    /// The credential's ID.
    pub credential_id: Vec<u8>,
    /// The authenticator data, with the attested credential.
    pub authenticator_data: Vec<u8>,
    /// The client data JSON, which carries the challenge and the origin.
    pub client_data: Vec<u8>,
    /// The credential's P-256 public key, DER-encoded.
    pub public_key: Vec<u8>,
}

/// What a passkey answered to a challenge, from `WebAuthn`'s `get()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasskeyAssertion {
    /// The authenticator data.
    pub authenticator_data: Vec<u8>,
    /// The client data JSON, which carries the challenge and the origin.
    pub client_data: Vec<u8>,
    /// The signature over both.
    pub signature: Vec<u8>,
}

/// The device ID of the passkey with this credential ID.
#[must_use]
pub fn passkey_device(credential_id: &[u8]) -> AccountId32 {
    AccountId32::from_bytes(Blake2b::<U32>::digest(credential_id).into())
}

/// A key device answering an [`Enrollment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceKey {
    /// The device's sr25519 public key, which becomes its device ID.
    pub public: AccountId32,
    /// Its signature over [`Enrollment::message`].
    pub signature: [u8; 64],
}

/// A session waiting for the device's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRequest {
    /// The user logging in.
    pub user: UserId,
    /// Their pass account, which the session will act for.
    pub account: AccountId32,
    /// The device asked to authorize it.
    pub device: AccountId32,
    /// The account of the session key.
    pub session: AccountId32,
    /// What a key device signs.
    pub message: Vec<u8>,
    /// What a passkey signs: the challenge of its `get()`.
    pub challenge: [u8; 32],
    /// The call and the transaction context the signature commits to.
    call: Vec<u8>,
    spec_version: u32,
    tx_version: u32,
    genesis: [u8; 32],
    nonce: u64,
    checkpoint: crate::Block,
    mortality: Mortality,
}

/// Records the challenge `PassAuthenticate` asks a device of this kind for,
/// and stops.
struct Asked(&'static str, RefCell<Option<ChallengeRequest>>);

impl DeviceAuthenticator for Asked {
    fn variant(&self) -> &'static str {
        self.0
    }

    async fn attest(&self, _: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        Err(sube::Error::Signing("not an enrollment".into()))
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        *self.1.borrow_mut() = Some(request.clone());
        Err(sube::Error::Signing("waiting for the device".into()))
    }
}

/// Answers the recorded challenge with the signature a device produced.
struct Answered {
    challenge: [u8; 32],
    signature: [u8; 64],
}

impl DeviceAuthenticator for Answered {
    fn variant(&self) -> &'static str {
        SUBSTRATE_KEY
    }

    async fn attest(&self, _: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        Err(sube::Error::Signing("not an enrollment".into()))
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        if request.challenge != self.challenge {
            return Err(sube::Error::Signing(
                "the transaction no longer matches what the device signed".into(),
            ));
        }
        // substrate-keys `KeySignature { user_id, message, signature }`.
        Ok(DynValue::obj(&[(
            SUBSTRATE_KEY,
            DynValue::obj(&[
                ("user_id", DynValue::from(request.user_id.0)),
                ("message", request.message_value()),
                ("signature", sr25519(&self.signature)),
            ]),
        )]))
    }
}

/// Answers the recorded challenge with what a passkey produced.
struct AnsweredPasskey<'a> {
    challenge: [u8; 32],
    assertion: &'a PasskeyAssertion,
}

impl DeviceAuthenticator for AnsweredPasskey<'_> {
    fn variant(&self) -> &'static str {
        WEBAUTHN
    }

    async fn attest(&self, _: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        Err(sube::Error::Signing("not an enrollment".into()))
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        if request.challenge != self.challenge {
            return Err(sube::Error::Signing(
                "the transaction no longer matches what the passkey signed".into(),
            ));
        }
        // webauthn `Assertion { meta, authenticator_data, client_data, signature }`.
        Ok(DynValue::obj(&[(
            WEBAUTHN,
            DynValue::obj(&[
                (
                    "meta",
                    DynValue::obj(&[
                        ("authority_id", DynValue::from(request.authority_id.0)),
                        ("user_id", DynValue::from(request.user_id.0)),
                        ("context", DynValue::from(request.context)),
                    ]),
                ),
                (
                    "authenticator_data",
                    DynValue::from(self.assertion.authenticator_data.clone()),
                ),
                (
                    "client_data",
                    DynValue::from(self.assertion.client_data.clone()),
                ),
                (
                    "signature",
                    DynValue::from(self.assertion.signature.clone()),
                ),
            ]),
        )]))
    }
}

fn sr25519(signature: &[u8; 64]) -> DynValue {
    DynValue::obj(&[("Sr25519", DynValue::from(signature.as_slice()))])
}

fn block_number(number: u64) -> Result<u32> {
    u32::try_from(number).map_err(|_| Error::Decode("block number above u32".into()))
}

impl State {
    fn key_devices(&self) -> Result<()> {
        if self.pass.supports_attestation(SUBSTRATE_KEY)
            && self.pass.supports_credential(SUBSTRATE_KEY)
        {
            Ok(())
        } else {
            Err(Error::Invalid(
                "the runtime does not accept key devices".into(),
            ))
        }
    }

    fn enrollment_request(&self, enrollment: &Enrollment) -> ChallengeRequest {
        ChallengeRequest::enrollment(
            HashedUserId(enrollment.user.0),
            Account(enrollment.account.into_bytes()),
            self.pass.authority_id,
            enrollment.block.number,
            &enrollment.block.hash,
        )
    }

    pub(crate) async fn enrollment(&mut self, user: UserId) -> Result<Enrollment> {
        self.key_devices()?;
        let mut enrollment = Enrollment {
            user,
            account: AccountId32::from_bytes(self.pass.derive_account(HashedUserId(user.0)).0),
            message: Vec::new(),
            challenge: [0; 32],
            block: self.head().await?,
        };
        let request = self.enrollment_request(&enrollment);
        enrollment.message = request.signed_message();
        enrollment.challenge = request.challenge;
        Ok(enrollment)
    }

    pub(crate) async fn register(
        &mut self,
        registrar: &Key,
        enrollment: &Enrollment,
        device: &DeviceKey,
    ) -> Result<Receipt> {
        let call = self.register_call(enrollment, device)?;
        self.submit(&call, &registrar.signer()).await
    }

    fn register_call(&self, enrollment: &Enrollment, device: &DeviceKey) -> Result<PreparedCall> {
        let attestation = self.attestation(enrollment, device)?;
        Ok(self.sube.prepare_call(
            &format!("{}/{}", self.pass.pallet, self.pass.register.name),
            &DynValue::obj(&[
                ("user", DynValue::from(enrollment.user.0)),
                ("attestation", attestation.runtime_value()),
            ]),
        )?)
    }

    /// Register the pass account of `enrollment` and give it `funds`, in one
    /// transaction: either both happen or neither.
    pub(crate) async fn register_funded(
        &mut self,
        registrar: &Key,
        enrollment: &Enrollment,
        device: &DeviceKey,
        funds: u128,
    ) -> Result<Receipt> {
        let register = self.register_call(enrollment, device)?;
        let fund = self.transfer_call(enrollment.account, funds)?;
        let call = self.batch(vec![register.bytes, fund.bytes])?;
        self.submit(&call, &registrar.signer()).await
    }

    /// `Utility.batch_all` of `calls`: all of them, or none.
    fn batch(&self, calls: Vec<Vec<u8>>) -> Result<PreparedCall> {
        // Its two index bytes come from an empty batch; the calls follow.
        let empty = self.sube.prepare_call(
            "utility/batch_all",
            &DynValue::obj(&[("calls", DynValue::Seq(Vec::new()))]),
        )?;
        let Some(indices) = empty.bytes.get(..2) else {
            return Err(Error::Decode("Utility.batch_all has no indices".into()));
        };
        let count = u32::try_from(calls.len())
            .map_err(|_| Error::Invalid("too many calls in a batch".into()))?;
        let mut bytes = indices.to_vec();
        Compact(count).encode_to(&mut bytes);
        bytes.extend(calls.into_iter().flatten());
        Ok(PreparedCall {
            pallet: empty.pallet,
            call: empty.call,
            bytes,
        })
    }

    /// The attestation of the key device that signed `enrollment`.
    fn attestation(
        &self,
        enrollment: &Enrollment,
        device: &DeviceKey,
    ) -> Result<DeviceAttestation> {
        self.key_devices()?;
        let request = self.enrollment_request(enrollment);
        if request.signed_message() != enrollment.message {
            return Err(Error::Invalid("the enrollment is not this chain's".into()));
        }
        // substrate-keys `KeyRegistration { public, message, signature }`.
        Ok(DeviceAttestation {
            device_id: DeviceId(device.public.into_bytes()),
            variant: SUBSTRATE_KEY.into(),
            payload: DynValue::obj(&[
                ("public", DynValue::from(device.public.into_bytes())),
                ("message", request.message_value()),
                ("signature", sr25519(&device.signature)),
            ]),
        })
    }

    /// What a session key may call: contracts, and nothing else.
    pub(crate) fn session_filter(&self) -> Result<DeviceFilter> {
        Ok(DeviceFilter::calls(self.sube.metadata(), [SESSION_CALL])?)
    }

    pub(crate) async fn session_request(
        &mut self,
        user: UserId,
        device: AccountId32,
        session: AccountId32,
    ) -> Result<SessionRequest> {
        let call = pass::session::prepare_add_session_key(
            self.sube.metadata(),
            &self.pass,
            session.into_bytes(),
            &self.session_filter()?,
            None,
        )?;
        self.device_request(SUBSTRATE_KEY, user, device, session, call.bytes)
            .await
    }

    /// Like [`State::session_request`], for the passkey `device`.
    pub(crate) async fn passkey_session_request(
        &mut self,
        user: UserId,
        device: AccountId32,
        session: AccountId32,
    ) -> Result<SessionRequest> {
        let call = pass::session::prepare_add_session_key(
            self.sube.metadata(),
            &self.pass,
            session.into_bytes(),
            &self.session_filter()?,
            None,
        )?;
        self.device_request(WEBAUTHN, user, device, session, call.bytes)
            .await
    }

    /// Start adding a passkey to the account of `enrollment`, which carried
    /// the challenge the passkey was created with: what the admin device
    /// `admin` must sign. The passkey may call contracts and open sessions,
    /// and nothing else.
    pub(crate) async fn passkey_addition_request(
        &mut self,
        admin: AccountId32,
        enrollment: &Enrollment,
        passkey: &PasskeyRegistration,
    ) -> Result<SessionRequest> {
        if !self.pass.supports_attestation(WEBAUTHN) {
            return Err(Error::Invalid(
                "the runtime does not accept passkeys".into(),
            ));
        }
        let request = self.enrollment_request(enrollment);
        if request.challenge != enrollment.challenge {
            return Err(Error::Invalid("the enrollment is not this chain's".into()));
        }
        let device = passkey_device(&passkey.credential_id);
        let device_id = DeviceId(device.into_bytes());
        // webauthn `Attestation { meta, authenticator_data, client_data, public_key }`.
        let attestation = DeviceAttestation {
            device_id,
            variant: WEBAUTHN.into(),
            payload: DynValue::obj(&[
                ("meta", request.registration_meta(device_id)),
                (
                    "authenticator_data",
                    DynValue::from(passkey.authenticator_data.clone()),
                ),
                ("client_data", DynValue::from(passkey.client_data.clone())),
                ("public_key", DynValue::from(passkey.public_key.clone())),
            ]),
        };
        let open_session = format!("{}/{}", self.pass.pallet, self.pass.add_session_key.name);
        let filter = DeviceFilter::calls(self.sube.metadata(), [SESSION_CALL, &open_session])?;
        let add = pass::workflow::prepare_add_device(
            self.sube.metadata(),
            &self.pass,
            &attestation,
            &filter,
            false,
        )?;
        self.device_request(SUBSTRATE_KEY, enrollment.user, admin, device, add.bytes)
            .await
    }

    /// Start removing `device` from `user`'s account: what the admin device
    /// `admin` must sign.
    pub(crate) async fn removal_request(
        &mut self,
        user: UserId,
        admin: AccountId32,
        device: AccountId32,
    ) -> Result<SessionRequest> {
        let remove = pass::workflow::prepare_remove_device(
            self.sube.metadata(),
            &self.pass,
            DeviceId(device.into_bytes()),
        )?;
        self.device_request(SUBSTRATE_KEY, user, admin, device, remove.bytes)
            .await
    }

    /// Start replacing `old` with the device that signed `enrollment`: one
    /// batch adds the new device as an admin device and removes the old one,
    /// in that order, since an account keeps an admin device at all times.
    pub(crate) async fn replacement_request(
        &mut self,
        old: AccountId32,
        enrollment: &Enrollment,
        new: &DeviceKey,
    ) -> Result<SessionRequest> {
        let attestation = self.attestation(enrollment, new)?;
        // `Pass.add_device` takes no filter and makes the device an admin
        // device, which is what replaces one.
        let add = self.sube.prepare_call(
            &format!("{}/add_device", self.pass.pallet),
            &DynValue::obj(&[("attestation", attestation.runtime_value())]),
        )?;
        let remove = pass::workflow::prepare_remove_device(
            self.sube.metadata(),
            &self.pass,
            DeviceId(old.into_bytes()),
        )?;
        let batch = self.batch(vec![add.bytes, remove.bytes])?.bytes;
        self.device_request(SUBSTRATE_KEY, enrollment.user, old, new.public, batch)
            .await
    }

    /// Assemble `call` as `user`'s pass account up to the point where
    /// `device` is asked to sign it.
    async fn device_request(
        &mut self,
        kind: &'static str,
        user: UserId,
        device: AccountId32,
        session: AccountId32,
        call: Vec<u8>,
    ) -> Result<SessionRequest> {
        if !self.pass.supports_credential(kind) {
            return Err(Error::Invalid(format!(
                "the runtime does not accept {kind} devices"
            )));
        }
        let account = self.pass.derive_account(HashedUserId(user.0));
        let metadata = self.sube.metadata_rc();
        // A device may answer minutes later, and a fork builds a block with
        // every transaction: the request stays valid for many blocks.
        let options = TransactionOptions {
            mortality: Mortality::Mortal {
                period: REQUEST_BLOCKS,
            },
            ..TransactionOptions::default()
        };
        let context =
            sube::extrinsic::build_context(self.sube.backend(), &metadata, &options, &account.0)
                .await?;
        let checkpoint = crate::Block {
            number: block_number(context.checkpoint_number)?,
            hash: context.checkpoint_hash,
        };

        // Assemble up to the point where the device is asked to sign.
        let asked = Asked(kind, RefCell::new(None));
        let authenticator = PassAuthenticator::new(
            &asked,
            account,
            DeviceId(device.into_bytes()),
            HashedUserId(user.0),
            self.pass.authority_id,
            checkpoint.number,
            checkpoint.hash,
        );
        let stopped = authenticator
            .assemble(
                &call,
                &metadata.extrinsic,
                &metadata.registry,
                &context,
                &[],
            )
            .await;
        let Some(request) = asked.1.into_inner() else {
            return Err(match stopped {
                Err(error) => error.into(),
                Ok(_) => Error::Decode("PassAuthenticate asked for no credential".into()),
            });
        };

        Ok(SessionRequest {
            user,
            account: AccountId32::from_bytes(account.0),
            device,
            session,
            message: request.signed_message(),
            challenge: request.challenge,
            call,
            spec_version: context.spec_version,
            tx_version: context.tx_version,
            genesis: context.genesis_hash,
            nonce: context.account_nonce,
            checkpoint,
            mortality: context.mortality,
        })
    }

    pub(crate) async fn open_session(
        &mut self,
        request: &SessionRequest,
        signature: [u8; 64],
    ) -> Result<Receipt> {
        let answered = Answered {
            challenge: request.challenge,
            signature,
        };
        self.submit_authorized(request, &answered).await
    }

    pub(crate) async fn open_passkey_session(
        &mut self,
        request: &SessionRequest,
        assertion: &PasskeyAssertion,
    ) -> Result<Receipt> {
        let answered = AnsweredPasskey {
            challenge: request.challenge,
            assertion,
        };
        self.submit_authorized(request, &answered).await
    }

    /// Submit the transaction of `request` with the device's answer.
    async fn submit_authorized(
        &mut self,
        request: &SessionRequest,
        answered: &impl DeviceAuthenticator,
    ) -> Result<Receipt> {
        let context = ChainContext {
            spec_version: request.spec_version,
            tx_version: request.tx_version,
            genesis_hash: request.genesis,
            account_nonce: request.nonce,
            checkpoint_number: u64::from(request.checkpoint.number),
            checkpoint_hash: request.checkpoint.hash,
            mortality: request.mortality,
            tip: 0,
        };
        let authenticator = PassAuthenticator::new(
            answered,
            Account(request.account.into_bytes()),
            DeviceId(request.device.into_bytes()),
            HashedUserId(request.user.0),
            self.pass.authority_id,
            request.checkpoint.number,
            request.checkpoint.hash,
        );
        let metadata = self.sube.metadata_rc();
        let assembled = authenticator
            .assemble(
                &request.call,
                &metadata.extrinsic,
                &metadata.registry,
                &context,
                &[],
            )
            .await?;
        let length = u32::try_from(assembled.bytes.len())
            .map_err(|_| Error::Invalid("extrinsic too large".into()))?;
        let extrinsic = [Compact(length).encode(), assembled.bytes].concat();
        let receipt = self
            .sube
            .backend()
            .submit(&extrinsic, WaitFor::Finalized, &metadata)
            .await?;
        State::receipt(receipt)
    }
}
