//! The chain thread: it owns the sube connection and runs one request at a time.

use core::future::Future;
use core::pin::Pin;

use pass::{Account, PassRuntimeConfig, SessionAuthorizer};
use protocol::AccountId32;
use sube::{
    Backend, DispatchOutcome, DynValue, ExtrinsicAssembler, PreparedCall, Sube, TransactionOptions,
    TransactionReceipt, WaitFor,
};
use tokio::sync::{mpsc, oneshot};

use crate::contracts::hex0x;
use crate::{Authority, Block, Error, Event, Info, Key, Receipt, Result, Weight};

/// A request for the chain thread: it borrows the state while it runs.
pub(crate) type Job =
    Box<dyn for<'a> FnOnce(&'a mut State) -> Pin<Box<dyn Future<Output = ()> + 'a>> + Send>;

/// Everything the chain thread owns.
pub(crate) struct State {
    pub(crate) sube: Sube,
    /// How this runtime configures pallet-pass, read from its metadata.
    pub(crate) pass: PassRuntimeConfig,
}

/// Connect, report through `ready`, then serve `queue` until every [`Chain`]
/// is gone.
///
/// [`Chain`]: crate::Chain
pub(crate) fn run(url: &str, mut queue: mpsc::Receiver<Job>, ready: oneshot::Sender<Result<()>>) {
    let mut state = match smol::block_on(State::connect(url)) {
        Ok(state) => state,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    while let Some(job) = queue.blocking_recv() {
        smol::block_on(job(&mut state));
    }
    tracing::debug!("chain thread stopped");
}

/// A key as a sube signer of V4 extrinsics.
pub(crate) struct KeySigner<'a>(&'a Key);

impl sube::Signer for KeySigner<'_> {
    type Account = [u8; 32];
    type Signature = [u8; 64];

    async fn sign(&self, data: impl AsRef<[u8]>) -> sube::Result<[u8; 64]> {
        self.0
            .sign(data.as_ref())
            .await
            .map_err(|error| sube::Error::Signing(error.to_string()))
    }

    fn account(&self) -> [u8; 32] {
        self.0.account().into_bytes()
    }
}

impl Key {
    /// This key as a sube signer of V4 extrinsics.
    pub(crate) const fn signer(&self) -> KeySigner<'_> {
        KeySigner(self)
    }
}

impl State {
    async fn connect(url: &str) -> Result<Self> {
        let sube = Box::pin(Sube::connect(url)).await?;
        let pass = PassRuntimeConfig::discover(sube.metadata())?;
        if sube.metadata().pallet_by_name("Contracts").is_none() {
            return Err(Error::Invalid("the runtime has no Contracts pallet".into()));
        }
        Ok(Self { sube, pass })
    }

    /// The latest finalized block.
    pub(crate) async fn head(&mut self) -> Result<Block> {
        let head = self.sube.backend().block_info(None).await?;
        Ok(Block {
            number: u32::try_from(head.number)
                .map_err(|_| Error::Decode("block number above u32".into()))?,
            hash: head.hash,
        })
    }

    pub(crate) async fn info(&mut self) -> Result<Info> {
        let (spec_version, _) = sube::extrinsic::runtime_versions(self.sube.metadata())?;
        Ok(Info {
            genesis: self.sube.backend().block_info(Some(0)).await?.hash,
            spec_version,
            head: self.head().await?,
            max_session_blocks: self.pass.max_session_duration,
            fork: self.is_fork(),
        })
    }

    #[cfg(feature = "fork")]
    fn is_fork(&self) -> bool {
        self.sube.fork().is_some()
    }

    #[cfg(not(feature = "fork"))]
    #[allow(clippy::unused_self, reason = "Mirrors the fork build's signature.")]
    const fn is_fork(&self) -> bool {
        false
    }

    /// Call a runtime API on the latest finalized block.
    pub(crate) async fn runtime_api(
        &mut self,
        function: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>> {
        let head = self.head().await?;
        Ok(self
            .sube
            .runtime_call_at(&hex0x(&head.hash), function, &hex0x(parameters))
            .await?)
    }

    /// Build `call` as `authority` would authorize it, submit it and wait for
    /// its block to be finalized.
    pub(crate) async fn submit(
        &mut self,
        call: &PreparedCall,
        authority: &(impl ExtrinsicAssembler + ?Sized),
    ) -> Result<Receipt> {
        let extrinsic = self
            .sube
            .build_transaction(call, authority, TransactionOptions::default())
            .await?;
        let receipt = self
            .sube
            .submit_transaction(&extrinsic, WaitFor::Finalized)
            .await?;
        Self::receipt(receipt)
    }

    pub(crate) fn receipt(receipt: TransactionReceipt) -> Result<Receipt> {
        let (Some(block), Some(index)) = (receipt.finalized_block_hash, receipt.extrinsic_index)
        else {
            return Err(Error::Decode("receipt without its finalized block".into()));
        };
        let failure = match receipt.dispatch_outcome {
            DispatchOutcome::Success => None,
            DispatchOutcome::Failed(reason) => Some(reason),
            // Included, but its events could not be read: the outcome is not known.
            DispatchOutcome::Unknown => {
                return Err(Error::Decode(format!(
                    "events of {block}#{index}: {}",
                    receipt.events_error.unwrap_or_default()
                )));
            }
        };
        Ok(Receipt {
            block,
            index,
            failure,
            events: receipt
                .events
                .into_iter()
                .map(|event| Event {
                    pallet: event.pallet,
                    variant: event.variant,
                    fields: event.decoded,
                })
                .collect(),
        })
    }

    pub(crate) async fn call(
        &mut self,
        authority: &Authority,
        contract: AccountId32,
        input: &[u8],
        gas: Weight,
    ) -> Result<Receipt> {
        let call = self.contracts_call(contract, input, gas)?;
        match authority {
            Authority::Key(key) => self.submit(&call, &key.signer()).await,
            Authority::Session { account, key } => {
                let session = SessionAuthorizer::new(
                    Account(account.into_bytes()),
                    key.signer(),
                    self.session_filter()?,
                )?;
                self.submit(&call, &session).await
            }
        }
    }

    pub(crate) async fn transfer(
        &mut self,
        from: &Key,
        to: AccountId32,
        amount: u128,
    ) -> Result<Receipt> {
        let call = self.sube.prepare_call(
            "balances/transfer_keep_alive",
            &DynValue::obj(&[
                (
                    "dest",
                    DynValue::obj(&[("Id", DynValue::from(to.into_bytes()))]),
                ),
                ("value", DynValue::U128(amount)),
            ]),
        )?;
        self.submit(&call, &from.signer()).await
    }
}
