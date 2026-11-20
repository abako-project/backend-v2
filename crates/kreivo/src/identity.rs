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
use sube::{Backend, DynValue, ExtrinsicAssembler, Mortality, TransactionOptions, WaitFor};

use crate::{Error, Key, Receipt, Result, worker::State};

/// The attestation and credential variant of key devices.
const SUBSTRATE_KEY: &str = "SubstrateKey";
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
    /// What the device signs.
    pub message: Vec<u8>,
    /// The block the challenge is bound to; the chain forgets old ones.
    pub block: crate::Block,
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
    /// What the device signs.
    pub message: Vec<u8>,
    /// The `Pass.add_session_key` call and the transaction context the
    /// signature commits to.
    call: Vec<u8>,
    challenge: [u8; 32],
    spec_version: u32,
    tx_version: u32,
    genesis: [u8; 32],
    nonce: u64,
    checkpoint: crate::Block,
    mortality: Mortality,
}

/// Records the challenge `PassAuthenticate` asks a device for, and stops.
struct Asked(RefCell<Option<ChallengeRequest>>);

impl DeviceAuthenticator for Asked {
    fn variant(&self) -> &'static str {
        SUBSTRATE_KEY
    }

    async fn attest(&self, _: &ChallengeRequest) -> sube::Result<DeviceAttestation> {
        Err(sube::Error::Signing("not an enrollment".into()))
    }

    async fn assert(&self, request: &ChallengeRequest) -> sube::Result<DynValue> {
        *self.0.borrow_mut() = Some(request.clone());
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
            block: self.head().await?,
        };
        enrollment.message = self.enrollment_request(&enrollment).signed_message();
        Ok(enrollment)
    }

    pub(crate) async fn register(
        &mut self,
        registrar: &Key,
        enrollment: &Enrollment,
        device: &DeviceKey,
    ) -> Result<Receipt> {
        self.key_devices()?;
        let request = self.enrollment_request(enrollment);
        if request.signed_message() != enrollment.message {
            return Err(Error::Invalid("the enrollment is not this chain's".into()));
        }
        // substrate-keys `KeyRegistration { public, message, signature }`.
        let attestation = DeviceAttestation {
            device_id: DeviceId(device.public.into_bytes()),
            variant: SUBSTRATE_KEY.into(),
            payload: DynValue::obj(&[
                ("public", DynValue::from(device.public.into_bytes())),
                ("message", request.message_value()),
                ("signature", sr25519(&device.signature)),
            ]),
        };
        let call = self.sube.prepare_call(
            &format!("{}/{}", self.pass.pallet, self.pass.register.name),
            &DynValue::obj(&[
                ("user", DynValue::from(enrollment.user.0)),
                ("attestation", attestation.runtime_value()),
            ]),
        )?;
        self.submit(&call, &registrar.signer()).await
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
        self.key_devices()?;
        let account = self.pass.derive_account(HashedUserId(user.0));
        let call = pass::session::prepare_add_session_key(
            self.sube.metadata(),
            &self.pass,
            session.into_bytes(),
            &self.session_filter()?,
            None,
        )?;
        let metadata = self.sube.metadata_rc();
        let context = sube::extrinsic::build_context(
            self.sube.backend(),
            &metadata,
            &TransactionOptions::default(),
            &account.0,
        )
        .await?;
        let checkpoint = crate::Block {
            number: block_number(context.checkpoint_number)?,
            hash: context.checkpoint_hash,
        };

        // Assemble up to the point where the device is asked to sign.
        let asked = Asked(RefCell::new(None));
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
                &call.bytes,
                &metadata.extrinsic,
                &metadata.registry,
                &context,
                &[],
            )
            .await;
        let Some(request) = asked.0.into_inner() else {
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
            call: call.bytes,
            challenge: request.challenge,
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
        let answered = Answered {
            challenge: request.challenge,
            signature,
        };
        let authenticator = PassAuthenticator::new(
            &answered,
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
