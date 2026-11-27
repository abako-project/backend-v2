//! The provider on a fork of Kreivo: its first start, its saved state, the
//! execution of a signed command, and the pass accounts and sessions that
//! authorize it.
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use kreivo::{Authority, Chain, DeviceKey, Key, Origin, UserId, Weight, message};
use protocol::provider_core::{
    CatalogEntryCore, CatalogReferenceV1, CoreSnapshot, SkillRoleCore, catalog_seed_v1,
};
use protocol::{
    AccountNonce, CatalogKind, OperationId, OperationReceipt, PayloadHash, ProviderEvents,
    ProviderInfo, SignedContractCallV1, UnixSeconds,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::routing::{self, Target};
use crate::transport::execution::{Admission, ExecutionInputs, Shell, VerifiedCall};
use crate::transport::{self, ProviderApi};
use crate::{Account, Error, State, reads};

/// One KSM, in planck.
const UNIT: u128 = 1_000_000_000_000;
/// What the onboarding account starts with on a fork.
const ONBOARDING_FUNDS: u128 = 10_000_000 * UNIT;
/// What a new pass account gets for fees and storage deposits on a fork.
const ACCOUNT_FUNDS: u128 = 1_000 * UNIT;
/// What the operator gets: it also pays for the contracts' code and storage.
const OPERATOR_FUNDS: u128 = 100_000 * UNIT;
/// The provider's local budget for the Dao constructor, which needs more
/// proof than a Kreivo block carries.
const CONSTRUCTOR_GAS: Weight = Weight {
    ref_time: 1_000_000_000_000,
    proof_size: 16 * 1024 * 1024,
};
/// A relay chain block, Kreivo's clock.
const RELAY_BLOCK_MS: u64 = 6_000;
/// What the community starts with on a fork: it pays its subscription and
/// the deposits of its memberships.
const COMMUNITY_FUNDS: u128 = 100_000 * UNIT;
/// The price of the community's subscription for one billing period.
const SUBSCRIPTION_PRICE: u128 = UNIT / 100;
/// The pool's allowance for one usage period, an hour of relay blocks.
const POOL_REF_TIME: u64 = 1_000_000_000_000_000;
const POOL_PROOF_SIZE: u64 = 10_000_000_000;
/// What Kreivo locks for one storage entry a contract call adds.
const DEPOSIT_PER_ENTRY: u128 = 6_666_666_660;
/// How many writes may run or wait for the writer at once.
const MAX_WRITES: usize = 64;
/// How many memberships the community issues at a time, the most one call
/// may.
const MEMBERSHIPS_PER_ISSUE: u32 = 256;

/// The account of the community with this ID on Kreivo: the communities
/// pallet's ID, then the community's.
fn community_account(id: u16) -> Account {
    let mut bytes = [0; 32];
    bytes[..4].copy_from_slice(b"modl");
    bytes[4..12].copy_from_slice(b"kv/cmtys");
    bytes[12..14].copy_from_slice(&id.to_le_bytes());
    Account::from_bytes(bytes)
}

/// The code of a command the chain included and failed after its dry-run
/// passed: it ran out of gas or of balance for its storage deposit.
const FAILED_ON_CHAIN: &str = "call_failed_on_chain";
/// The longest stretch the fork's clock is moved in one request.
const CLOCK_STEP_MS: u64 = 24 * 60 * 60 * 1000;
/// A session is not handed out again once it has less than this left.
const REUSE_ABOVE_MS: u64 = 10 * 60 * 1000;

/// A passkey login under way: the transaction that opens the session, and
/// the session's key.
#[derive(Debug)]
pub struct PasskeyLogin {
    request: kreivo::SessionRequest,
    key: Key,
}

impl PasskeyLogin {
    /// The challenge the passkey must answer.
    #[must_use]
    pub fn challenge(&self) -> [u8; 32] {
        self.request.challenge
    }
}

/// How the provider is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// A snapshot of Kreivo to fork on the first start.
    pub snapshot: PathBuf,
    /// Where the fork and the provider's state are saved.
    pub data_dir: PathBuf,
    /// The directory of `dao.contract` and `project.contract`.
    pub contracts_dir: PathBuf,
    /// Whether the Dao accepts development funding.
    pub mock_funding: bool,
    /// The username of the operator's pass account.
    pub operator_user: String,
    /// The ID of a community to set up on the fork, with a usage
    /// subscription, and to make every account a member of, so that fees come
    /// from its pool as on Kreivo. Without one, accounts pay their own fees.
    pub community: Option<u16>,
}

/// A pass session the provider can sign with.
struct Session {
    key: Key,
    /// The device that opened it.
    device: Account,
    /// When it stops being used, in milliseconds since the Unix epoch.
    until_ms: u64,
}

struct Inner {
    chain: Chain,
    /// Whoever holds this lock is the one writer to the chain and the state.
    writer: tokio::sync::Mutex<()>,
    /// How many more writes may wait for the writer.
    writes: Arc<tokio::sync::Semaphore>,
    /// The state readers see: replaced whole by the writer.
    state: RwLock<Arc<State>>,
    /// Whether the contracts are set up, so that what is committed is saved.
    ready: std::sync::atomic::AtomicBool,
    sessions: Mutex<BTreeMap<Account, Session>>,
    /// The latest snapshot, with the block it was read at.
    snapshot: Mutex<Option<(u32, CoreSnapshot)>>,
    onboarding: Key,
    operator: Account,
    /// The account of the community every account joins, if there is one.
    community: Option<Account>,
    data_dir: PathBuf,
    session_ms: u64,
    router: OnceLock<axum::Router>,
    token: String,
}

/// The provider. Cheap to clone.
#[derive(Clone)]
pub struct Provider(Arc<Inner>);

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider").finish_non_exhaustive()
    }
}

fn now_ms() -> Result<u64, Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Invariant("the clock is before 1970"))?;
    u64::try_from(now.as_millis()).map_err(|_| Error::Limit)
}

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Invariant("no entropy"))?;
    Ok(bytes)
}

fn code(directory: &Path, name: &str) -> Result<Vec<u8>, Error> {
    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join(format!("{name}.contract")))?)
            .map_err(|_| Error::InvalidState)?;
    let hex = bundle["source"]["wasm"]
        .as_str()
        .and_then(|hex| hex.strip_prefix("0x"))
        .ok_or(Error::InvalidState)?;
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            hex.get(at..at + 2)
                .and_then(|byte| u8::from_str_radix(byte, 16).ok())
                .ok_or(Error::InvalidState)
        })
        .collect()
}

/// The catalog the Dao starts with, as the embedded provider seeds it.
fn catalog(
    generation: protocol::ProviderInstanceId,
) -> (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
) {
    let mut catalog = (Vec::new(), Vec::new(), Vec::new());
    for seed in catalog_seed_v1() {
        let hash: [u8; 32] = Sha256::digest(seed.record(generation).hash_input()).into();
        let content = CatalogReferenceV1::Seed(PayloadHash::from_bytes(hash));
        let entry = CatalogEntryCore {
            id: seed.id,
            fixed: seed.kind == CatalogKind::Role && seed.id == 1,
            content: content.clone(),
        };
        match seed.kind {
            CatalogKind::Role => catalog.0.push(entry),
            CatalogKind::Skill => {
                catalog.1.push(entry);
                catalog.2.push(SkillRoleCore {
                    skill_id: seed.id,
                    role_ids: seed.role_ids.to_vec(),
                    origin: content,
                });
            }
        }
    }
    catalog
}

fn succeeded(receipt: &kreivo::Receipt, what: &'static str) -> Result<(), Error> {
    match &receipt.failure {
        None => Ok(()),
        Some(failure) => {
            tracing::error!(what, failure, "a setup transaction failed");
            Err(Error::Invariant(what))
        }
    }
}

fn load_or_create_key(path: &Path) -> Result<Key, Error> {
    let seed = if path.exists() {
        let bytes = Zeroizing::new(std::fs::read(path)?);
        let seed: [u8; 32] = bytes[..].try_into().map_err(|_| Error::InvalidState)?;
        Zeroizing::new(seed)
    } else {
        let seed = Zeroizing::new(random::<32>()?);
        write_private(path, &seed[..])?;
        seed
    };
    Ok(Key::from_seed(&seed)?)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::io::Write;
    let partial = path.with_extension("partial");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&partial, path)?;
    Ok(())
}

impl Provider {
    /// Start the provider: resume the saved fork, or fork the snapshot and
    /// set up the contracts with `operator_device` as the first device of the
    /// operator's pass account.
    ///
    /// # Errors
    /// Fails if the chain, the contract bundles or the saved state cannot be
    /// used, if the saved state belongs to another operator, or if a first
    /// start is given no device.
    pub async fn open(config: Config, operator_device: Option<&Key>) -> Result<Self, Error> {
        std::fs::create_dir_all(&config.data_dir)?;
        let fork_path = config.data_dir.join("fork.snap");
        let state_path = config.data_dir.join("state.json");
        let resumed = Self::saved(&config.data_dir);
        let source = if resumed {
            &fork_path
        } else {
            &config.snapshot
        };
        let chain = Chain::connect(&format!("fork:{}", source.display())).await?;
        let info = chain.info().await?;
        let onboarding = load_or_create_key(&config.data_dir.join("onboarding.seed"))?;
        // A snapshot is as old as the day it was taken. The runtime works
        // through every relay block the clock skips: go a day at a time.
        let now = now_ms()?;
        let mut at = chain.mock().read(|reader| reader.timestamp()).await??;
        while at + CLOCK_STEP_MS < now {
            at += CLOCK_STEP_MS;
            chain.mock().follow_clock(at).await?;
        }
        chain.mock().follow_clock(now).await?;
        let operator = chain
            .enrollment(UserId::of(&config.operator_user))
            .await?
            .account;

        let provider = Self(Arc::new(Inner {
            chain,
            writer: tokio::sync::Mutex::new(()),
            writes: Arc::new(tokio::sync::Semaphore::new(MAX_WRITES)),
            state: RwLock::new(Arc::new(State {
                version: 1,
                shell: Shell::new(operator)?,
                dao: operator,
                sessions: Vec::new(),
                community: config.community,
            })),
            ready: std::sync::atomic::AtomicBool::new(resumed),
            sessions: Mutex::new(BTreeMap::new()),
            snapshot: Mutex::new(None),
            onboarding,
            operator,
            community: config.community.map(community_account),
            data_dir: config.data_dir.clone(),
            session_ms: u64::from(info.max_session_blocks) * RELAY_BLOCK_MS,
            router: OnceLock::new(),
            token: random::<32>()?
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }));
        let _writer = provider.0.writer.lock().await;
        if resumed {
            let state: State = serde_json::from_slice(&std::fs::read(&state_path)?)
                .map_err(|_| Error::InvalidState)?;
            if state.version != 1 {
                return Err(Error::InvalidState);
            }
            state.shell.validate(operator)?;
            // The community is set up at the first start only.
            if state.community != config.community {
                return Err(Error::Invariant(
                    "the saved fork was set up with another community, or with none",
                ));
            }
            provider.replace(state)?;
            // The keys of the sessions open at the last stop are gone.
            provider.end_sessions(u64::MAX).await?;
        } else {
            let device = operator_device.ok_or(Error::Invariant(
                "a first start needs the operator's device",
            ))?;
            provider.first_start(&config, device).await?;
        }
        drop(_writer);
        Ok(provider)
    }

    async fn first_start(&self, config: &Config, device: &Key) -> Result<(), Error> {
        let inner = &self.0;
        let chain = &inner.chain;
        let onboarding = &inner.onboarding;
        chain
            .mock()
            .endow(onboarding.account(), ONBOARDING_FUNDS)
            .await?;

        if let Some(community) = config.community {
            self.subscribed_community(community).await?;
        }

        // The operator's account is registered, funded and then deploys the
        // Dao, which makes it the Dao's operator. An address that holds a
        // balance before it is registered cannot become a pass account.
        let operator = self
            .register_account(&config.operator_user, device, OPERATOR_FUNDS)
            .await?;
        let mut state = State {
            version: 1,
            shell: Shell::new(operator)?,
            dao: operator,
            sessions: Vec::new(),
            community: config.community,
        };
        let generation = state.shell.info.provider_instance_id;
        state.dao = chain
            .mock()
            .deploy(
                operator,
                code(&config.contracts_dir, "dao")?,
                message("new", (operator, operator, catalog(generation))),
                b"digesta".to_vec(),
                CONSTRUCTOR_GAS,
            )
            .await?;
        let project_code = chain
            .mock()
            .upload(operator, code(&config.contracts_dir, "project")?)
            .await?;

        // The payment asset and the Dao's inventory of it, as the embedded
        // provider sets them up.
        chain.mock().create_asset(onboarding.account()).await?;
        succeeded(
            &chain
                .mint(onboarding.clone(), state.dao, u128::from(u64::MAX) * 2)
                .await?,
            "mint",
        )?;

        let (authority, opened) = self.new_session(&config.operator_user, device).await?;
        state.sessions.push(opened);
        for input in [
            message("configure_project_code", project_code),
            message("configure_accounting", config.mock_funding),
        ] {
            let run = chain.dry_run(operator, state.dao, input.clone()).await?;
            let value = run.outcome.map_err(|_| Error::InvalidState)?;
            if value.reverted {
                return Err(Error::Invariant("the Dao refused its configuration"));
            }
            succeeded(
                &chain
                    .call(
                        authority.clone(),
                        state.dao,
                        input,
                        run.gas_required.with_margin(),
                    )
                    .await?,
                "configure",
            )?;
        }
        inner
            .ready
            .store(true, std::sync::atomic::Ordering::Release);
        self.commit(state, true).await
    }

    /// The state as it is now.
    fn current(&self) -> Result<Arc<State>, Error> {
        Ok(self
            .0
            .state
            .read()
            .map_err(|_| Error::Invariant("state lock"))?
            .clone())
    }

    /// Make `state` the state readers see.
    fn replace(&self, state: State) -> Result<Arc<State>, Error> {
        let state = Arc::new(state);
        *self
            .0
            .state
            .write()
            .map_err(|_| Error::Invariant("state lock"))? = state.clone();
        Ok(state)
    }

    /// Make `state` current, then save it and, when the chain changed, the
    /// fork. Only the writer calls this. Nothing is saved until the first
    /// start is over: a provider half set up must not be resumed.
    async fn commit(&self, state: State, chain_changed: bool) -> Result<(), Error> {
        let state = self.replace(state)?;
        if !self.0.ready.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        if chain_changed {
            self.save_fork().await?;
        }
        let path = self.0.data_dir.join("state.json");
        tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&*state).map_err(|_| Error::InvalidState)?;
            if bytes.len() > crate::MAX_SHELL {
                return Err(Error::Limit);
            }
            write_private(&path, &bytes)
        })
        .await
        .map_err(|_| Error::Invariant("the state was not saved"))?
    }

    async fn save_fork(&self) -> Result<(), Error> {
        Ok(self
            .0
            .chain
            .mock()
            .save(self.0.data_dir.join("fork.snap"))
            .await?)
    }

    /// Whether `data_dir` holds a provider to resume: one whose first start
    /// is over.
    #[must_use]
    pub fn saved(data_dir: &std::path::Path) -> bool {
        data_dir.join("fork.snap").exists() && data_dir.join("state.json").exists()
    }

    /// The operator's pass account, which is the Dao's operator.
    #[must_use]
    pub fn operator(&self) -> Account {
        self.0.operator
    }

    /// The wallet ID that stands for the operator's account in the wallet's
    /// interface.
    #[must_use]
    pub fn system_wallet(&self) -> protocol::WalletId {
        let mut id = [0; 16];
        id.copy_from_slice(&self.0.operator.into_bytes()[..16]);
        protocol::WalletId::from_bytes(id)
    }

    /// The native balance `account` can spend.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn balance(&self, account: Account) -> Result<u128, Error> {
        Ok(self.0.chain.mock().free_balance(account).await?)
    }

    /// The pass account of `username`. Nothing is registered.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn account(&self, username: &str) -> Result<Account, Error> {
        Ok(self.0.chain.enrollment(UserId::of(username)).await?.account)
    }

    /// Run a write to its end even if whoever asked for it stops waiting: a
    /// transaction that was sent must leave its mark here. A failure is
    /// logged, since nobody may be waiting for it.
    ///
    /// # Errors
    /// `capacity_exceeded` if too many writes are waiting already.
    async fn detached<T, F>(&self, work: F) -> Result<T, Error>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, Error>> + Send + 'static,
    {
        let permit = self
            .0
            .writes
            .clone()
            .try_acquire_owned()
            .map_err(|_| capacity_exceeded())?;
        tokio::spawn(async move {
            let _permit = permit;
            let result = work.await;
            match &result {
                Ok(_) | Err(Error::Provider(_)) => {}
                Err(error) => tracing::warn!(%error, "a chain write failed"),
            }
            result
        })
        .await
        .map_err(|_| Error::Invariant("a write was interrupted"))?
    }

    /// Register the pass account of `username` with `device` as its first
    /// device, and give it a balance for fees and storage deposits.
    ///
    /// # Errors
    /// `username_taken` if that pass account already exists on the chain.
    pub async fn register(&self, username: &str, device: &Key) -> Result<Account, Error> {
        let (provider, username, device) = (self.clone(), username.to_owned(), device.clone());
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            provider.0.chain.mock().follow_clock(now_ms()?).await?;
            let account = provider
                .register_account(&username, &device, ACCOUNT_FUNDS)
                .await?;
            provider.save_fork().await?;
            Ok(account)
        })
        .await
    }

    /// One transaction registers the account and funds it: either both
    /// happen or neither.
    async fn register_account(
        &self,
        username: &str,
        device: &Key,
        funds: u128,
    ) -> Result<Account, Error> {
        let chain = &self.0.chain;
        let enrollment = chain.enrollment(UserId::of(username)).await?;
        let account = enrollment.account;
        let signed = DeviceKey {
            public: device.account(),
            signature: device.sign(&enrollment.message),
        };
        let registered = chain
            .register_funded(self.0.onboarding.clone(), enrollment, signed, funds)
            .await?;
        match registered.failure.as_deref() {
            None => {}
            Some("Pass::AccountAlreadyRegistered") => {
                return Err(Error::Provider(transport::Error::domain("username_taken")));
            }
            Some(failure) => {
                tracing::error!(failure, "pass registration failed");
                return Err(Error::Invariant("pass registration failed"));
            }
        }
        // The account exists whatever follows: one that could not be
        // admitted pays its own fees from the balance it was given.
        if let Some(community) = self.0.community
            && let Err(error) = self.admit(community, account).await
        {
            tracing::error!(%error, %account, "the account was not admitted to the community");
        }
        Ok(account)
    }

    /// What governance and a community do on Kreivo, done on the fork with
    /// calls no account could sign: the community, with funds; a
    /// usage-subscription offer; the community's subscription to it; and
    /// memberships to give.
    async fn subscribed_community(&self, community: u16) -> Result<(), Error> {
        let mock = self.0.chain.mock();
        let account = community_account(community);
        // The name is "Digesta".
        mock.dispatch(
            Origin::Root,
            "communitiesManager/register".into(),
            format!(
                "(community_id:{community};name:0x44696765737461;first_admin:MultiAddress::Id({});\
                 maybe_decision_method:Option::None;maybe_track_info:Option::None)",
                self.0.onboarding.account(),
            ),
        )
        .await?;
        mock.dispatch(
            Origin::Root,
            "balances/force_set_balance".into(),
            format!("(who:MultiAddress::Id({account});new_free:{COMMUNITY_FUNDS})"),
        )
        .await?;
        // The offer takes the next ID: the snapshot may have offers already.
        let next = mock
            .storage_value("UsageSubscription".into(), "NextOfferId".into())
            .await?
            .unwrap_or_default();
        let mut offer = [0; 16];
        offer
            .get_mut(..next.len())
            .ok_or(Error::Invariant("the next offer's ID is not a number"))?
            .copy_from_slice(&next);
        let offer = u128::from_le_bytes(offer);
        // An hour's allowance far above what a fork's blocks of an hour use.
        mock.dispatch(
            Origin::Root,
            "usageSubscription/publish_offer".into(),
            format!(
                "(kind:OfferKind::Standard;terms:(allowance:(ref_time:{POOL_REF_TIME};proof_size:{POOL_PROOF_SIZE});\
                 usage_period:600;price:(asset:FungibleAssetLocation::Here(0);amount:{SUBSCRIPTION_PRICE});\
                 billing_period:432000;term:Option::None;min_commitment:Option::None;grace:43200))"
            ),
        )
        .await?;
        mock.dispatch(
            Origin::Signed(account),
            "usageSubscription/subscribe".into(),
            format!("(offer:{offer};converts_into:Option::None)"),
        )
        .await?;
        self.issue_memberships(account).await
    }

    async fn issue_memberships(&self, community: Account) -> Result<(), Error> {
        Ok(self
            .0
            .chain
            .mock()
            .dispatch(
                Origin::Signed(community),
                "communitiesManager/issue_memberships".into(),
                format!("(amount:{MEMBERSHIPS_PER_ISSUE})"),
            )
            .await?)
    }

    /// Make `account` a member of the community, as its admin does on Kreivo.
    /// A community out of memberships issues more.
    async fn admit(&self, community: Account, account: Account) -> Result<(), Error> {
        let mock = self.0.chain.mock();
        let member = format!("(who:MultiAddress::Id({account}))");
        let add = || {
            mock.dispatch(
                Origin::Signed(community),
                "communities/add_member".into(),
                member.clone(),
            )
        };
        match add().await {
            Err(kreivo::Error::Rejected(reason)) if reason.ends_with("::CommunityAtCapacity") => {
                self.issue_memberships(community).await?;
                Ok(add().await?)
            }
            added => Ok(added?),
        }
    }

    /// Whether `device` is a device of `username`'s pass account now.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn has_device(&self, username: &str, device: Account) -> Result<bool, Error> {
        let account = self.account(username).await?;
        Ok(self.0.chain.mock().has_device(account, device).await?)
    }

    /// Open a session for `username` with their `device`, or reuse the one
    /// that device already opened if it has long enough left. Returns the
    /// pass account and when the session ends, in seconds since the Unix
    /// epoch.
    ///
    /// # Errors
    /// `invalid_credentials` if the chain does not accept the device.
    pub async fn open_session(
        &self,
        username: &str,
        device: &Key,
    ) -> Result<(Account, UnixSeconds), Error> {
        let (provider, username, device) = (self.clone(), username.to_owned(), device.clone());
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            provider.0.chain.mock().follow_clock(now_ms()?).await?;
            let account = provider.account(&username).await?;
            let reusable = now_ms()? + REUSE_ABOVE_MS;
            let reused = provider.sessions()?.get(&account).and_then(|session| {
                (session.device == device.account() && session.until_ms > reusable)
                    .then_some(session.until_ms)
            });
            if let Some(until) = reused {
                return Ok((account, UnixSeconds::new(until / 1000)));
            }
            let (_, opened) = provider.new_session(&username, &device).await?;
            Ok((account, UnixSeconds::new(opened.2 / 1000)))
        })
        .await
    }

    /// End, on the chain and here, the sessions whose time has come by
    /// `now_ms`. Only the writer calls this.
    async fn end_sessions(&self, now_ms: u64) -> Result<(), Error> {
        let current = self.current()?;
        if !current.sessions.iter().any(|(.., until)| *until <= now_ms) {
            return Ok(());
        }
        let mut state = (*current).clone();
        for (account, key, _) in state
            .sessions
            .extract_if(.., |(.., until)| *until <= now_ms)
        {
            self.0.chain.mock().end_session(key).await?;
            let mut sessions = self.sessions()?;
            if sessions
                .get(&account)
                .is_some_and(|session| session.key.account() == key)
            {
                sessions.remove(&account);
            }
        }
        self.commit(state, true).await
    }

    /// Replace `old`, a device of `username`'s pass account, with `new`, as
    /// when a password changes. Open sessions stay open.
    ///
    /// # Errors
    /// `invalid_credentials` if the chain does not accept `old`: nothing
    /// changed. Any other error leaves it unknown whether the device
    /// changed: [`Provider::has_device`] tells.
    pub async fn replace_device(&self, username: &str, old: &Key, new: &Key) -> Result<(), Error> {
        let (provider, username) = (self.clone(), username.to_owned());
        let (old, new) = (old.clone(), new.clone());
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            let chain = &provider.0.chain;
            chain.mock().follow_clock(now_ms()?).await?;
            let enrollment = chain.enrollment(UserId::of(&username)).await?;
            let signed = DeviceKey {
                public: new.account(),
                signature: new.sign(&enrollment.message),
            };
            let request = chain
                .replacement_request(old.account(), enrollment, signed)
                .await?;
            let account = request.account;
            provider
                .authorized(request, &old, "the device was not replaced")
                .await?;
            // The account's session was opened by the device that is gone.
            if let Some(session) = provider.sessions()?.get_mut(&account) {
                session.device = new.account();
            }
            Ok(())
        })
        .await
    }

    /// The challenge a new passkey of `username` is created with.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn passkey_enrollment(&self, username: &str) -> Result<kreivo::Enrollment, Error> {
        Ok(self.0.chain.enrollment(UserId::of(username)).await?)
    }

    /// Add `passkey` to the pass account of `enrollment`, authorized by its
    /// admin device `admin`. The passkey may call contracts and open
    /// sessions. Returns the passkey's device ID.
    ///
    /// # Errors
    /// `invalid_credentials` if the chain does not accept `admin`.
    pub async fn add_passkey(
        &self,
        admin: &Key,
        enrollment: kreivo::Enrollment,
        passkey: kreivo::PasskeyRegistration,
    ) -> Result<Account, Error> {
        let (provider, admin) = (self.clone(), admin.clone());
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            let chain = &provider.0.chain;
            chain.mock().follow_clock(now_ms()?).await?;
            let device = kreivo::passkey_device(&passkey.credential_id);
            let request = chain
                .passkey_addition_request(admin.account(), enrollment, passkey)
                .await?;
            provider
                .authorized(request, &admin, "the passkey was not added")
                .await?;
            Ok(device)
        })
        .await
    }

    /// Remove the device `device` from `username`'s pass account, authorized
    /// by its admin device `admin`. A device that is already gone is removed.
    ///
    /// # Errors
    /// `invalid_credentials` if the chain does not accept `admin`.
    pub async fn remove_device(
        &self,
        username: &str,
        admin: &Key,
        device: Account,
    ) -> Result<(), Error> {
        let (provider, username, admin) = (self.clone(), username.to_owned(), admin.clone());
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            let chain = &provider.0.chain;
            chain.mock().follow_clock(now_ms()?).await?;
            if !provider.has_device(&username, device).await? {
                return Ok(());
            }
            let request = chain
                .removal_request(UserId::of(&username), admin.account(), device)
                .await?;
            provider
                .authorized(request, &admin, "the device was not removed")
                .await
        })
        .await
    }

    /// Submit a request the key device `admin` authorizes, and save the fork.
    async fn authorized(
        &self,
        request: kreivo::SessionRequest,
        admin: &Key,
        what: &'static str,
    ) -> Result<(), Error> {
        let signature = admin.sign(&request.message);
        let done = self
            .0
            .chain
            .open_session(request, signature)
            .await
            .map_err(refused)?;
        if let Some(failure) = &done.failure {
            return Err(dispatch_failed(failure, what));
        }
        self.save_fork().await
    }

    /// Start a login with the passkey `device` of `username`: the challenge
    /// the passkey must answer, with the session it would open.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn passkey_login(
        &self,
        username: &str,
        device: Account,
    ) -> Result<PasskeyLogin, Error> {
        let key = Key::from_seed(&Zeroizing::new(random::<32>()?))?;
        let request = self
            .0
            .chain
            .passkey_session_request(UserId::of(username), device, key.account())
            .await?;
        Ok(PasskeyLogin { request, key })
    }

    /// Open the session of `login` with the passkey's answer. Returns the
    /// pass account and when the session ends, in seconds since the Unix
    /// epoch.
    ///
    /// # Errors
    /// `invalid_credentials` if the chain does not verify the answer, or if
    /// the login is too old to be submitted.
    pub async fn open_passkey_session(
        &self,
        login: PasskeyLogin,
        assertion: kreivo::PasskeyAssertion,
    ) -> Result<(Account, UnixSeconds), Error> {
        let provider = self.clone();
        self.detached(async move {
            let _writer = provider.0.writer.lock().await;
            let chain = &provider.0.chain;
            chain.mock().follow_clock(now_ms()?).await?;
            let PasskeyLogin { request, key } = login;
            let (account, device) = (request.account, request.device);
            let noted = provider.note(account, &key).await?;
            let opened = chain
                .open_passkey_session(request, assertion)
                .await
                .map_err(refused);
            let (_, opened) = provider.opened(opened, noted, device, key).await?;
            Ok((account, UnixSeconds::new(opened.2 / 1000)))
        })
        .await
    }

    /// Open a session on the chain with a key device and keep its key. Only
    /// the writer calls this.
    async fn new_session(
        &self,
        username: &str,
        device: &Key,
    ) -> Result<(Authority, (Account, Account, u64)), Error> {
        let chain = &self.0.chain;
        let key = Key::from_seed(&Zeroizing::new(random::<32>()?))?;
        let request = chain
            .session_request(UserId::of(username), device.account(), key.account())
            .await?;
        let account = request.account;
        let signature = device.sign(&request.message);
        let noted = self.note(account, &key).await?;
        let opened = chain
            .open_session(request, signature)
            .await
            .map_err(refused);
        self.opened(opened, noted, device.account(), key).await
    }

    /// Note that a session of `account` with this key is about to be opened,
    /// before anything is sent. The chain frees a session's place only when
    /// asked, so a session nobody remembers would hold its place for good.
    async fn note(&self, account: Account, key: &Key) -> Result<(Account, Account, u64), Error> {
        let noted = (account, key.account(), now_ms()? + self.0.session_ms);
        let mut state = (*self.current()?).clone();
        state.sessions.push(noted);
        self.commit(state, false).await?;
        Ok(noted)
    }

    /// Finish opening the session `noted`. The chain would end it after its
    /// relay blocks, which a fork runs through as fast as it builds blocks:
    /// its removal is cancelled and the provider ends it by the wall clock.
    /// The account's earlier sessions give their places back. If the session
    /// was not opened, its note goes once the chain has been told to end it.
    async fn opened(
        &self,
        opened: Result<kreivo::Receipt, Error>,
        noted: (Account, Account, u64),
        device: Account,
        key: Key,
    ) -> Result<(Authority, (Account, Account, u64)), Error> {
        let (account, session, until_ms) = noted;
        let mock = self.0.chain.mock();
        // A transaction the chain would not take, or took and failed, opened
        // nothing. After any other error the session may be there.
        let (refusal, unopened) = match opened {
            Ok(receipt) => (
                receipt
                    .failure
                    .map(|failure| dispatch_failed(&failure, "the session was not opened")),
                true,
            ),
            Err(error) => {
                let unopened = matches!(
                    error,
                    Error::Provider(_) | Error::Chain(kreivo::Error::Rejected(_))
                );
                (Some(error), unopened)
            }
        };
        if let Some(error) = refusal {
            // Ending a session that never opened changes nothing.
            if unopened || mock.end_session(session).await.is_ok() {
                let mut state = (*self.current()?).clone();
                state.sessions.retain(|(_, noted, _)| *noted != session);
                if let Err(unsaved) = self.commit(state, !unopened).await {
                    tracing::warn!(error = %unsaved, "a session's note was not dropped");
                }
            }
            return Err(error);
        }
        mock.keep_session(session).await?;
        self.sessions()?.insert(
            account,
            Session {
                key: key.clone(),
                device,
                until_ms,
            },
        );
        let mut state = (*self.current()?).clone();
        for (_, earlier, _) in state.sessions.extract_if(.., |(owner, other, _)| {
            *owner == account && *other != session
        }) {
            mock.end_session(earlier).await?;
        }
        self.commit(state, true).await?;
        Ok((Authority::Session { account, key }, noted))
    }

    fn sessions(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<Account, Session>>, Error> {
        self.0
            .sessions
            .lock()
            .map_err(|_| Error::Invariant("sessions lock"))
    }

    /// When `account`'s session ends, if it is still usable at `at_ms`.
    fn live_until(&self, account: Account, at_ms: u64) -> Option<u64> {
        let sessions = self.sessions().ok()?;
        let session = sessions.get(&account)?;
        (session.until_ms > at_ms).then_some(session.until_ms)
    }

    /// Whether `account` has a session the provider can sign with now.
    ///
    /// # Errors
    /// Fails if the clock cannot be read.
    pub fn has_session(&self, account: Account) -> Result<bool, Error> {
        Ok(self.live_until(account, now_ms()?).is_some())
    }

    /// When `account`'s session ends, in seconds since the Unix epoch, if it
    /// has one. A session that just ran out may still be reported.
    #[must_use]
    pub fn session_until(&self, account: Account) -> Option<UnixSeconds> {
        self.live_until(account, 0)
            .map(|until| UnixSeconds::new(until / 1000))
    }

    async fn chain_ms(&self) -> Result<u64, Error> {
        Ok(self
            .0
            .chain
            .mock()
            .read(|reader| reader.timestamp())
            .await??)
    }

    /// Bring the fork's clock to the wall clock and end the sessions whose
    /// time has come. Called now and then, so that weeks turn and sessions
    /// end while nothing is submitted.
    ///
    /// # Errors
    /// Fails if the chain cannot be reached.
    pub async fn tick(&self) -> Result<(), Error> {
        let _writer = self.0.writer.lock().await;
        let now = now_ms()?;
        self.0.chain.mock().follow_clock(now).await?;
        self.end_sessions(now).await
    }

    /// Execute a command: dry-run it, and if the contracts accept it, submit
    /// it with the origin's session key. Its receipt is worked out before
    /// anything is sent, and recorded whatever the chain then does with it.
    async fn run(&self, signed: SignedContractCallV1) -> Result<OperationReceipt, Error> {
        let verified = VerifiedCall::accept(signed)?;
        let chain = &self.0.chain;
        let _writer = self.0.writer.lock().await;
        let original = self.current()?;
        let wall = now_ms()?;
        chain.mock().follow_clock(wall).await?;
        // The block that includes the call lands one relay block after the
        // head: that is the contract's time, and the receipt's.
        let block_ms = self.chain_ms().await? + RELAY_BLOCK_MS;
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(block_ms / 1000),
            seed: random::<32>()?,
        };
        let admission =
            original
                .shell
                .prepare_at(verified, inputs, UnixSeconds::new(wall / 1000))?;
        let pending = match admission {
            Admission::Replay(receipt) => return Ok(receipt),
            Admission::Execute(pending) => *pending,
        };
        let origin = pending.call().origin;
        // Nothing is recorded for an account that could not have signed it.
        let key = self
            .sessions()?
            .get(&origin)
            .filter(|session| session.until_ms > wall)
            .map(|session| session.key.clone())
            .ok_or(Error::Provider(transport::Error::bad("session_expired")))?;
        let command = pending.call().command.clone();
        let event_cursor = original.shell.events.last().map_or(0, |event| event.cursor);
        let dao = original.dao;
        let mut state = (*original).clone();

        let (contract, input) = match routing::route(&command, pending.inputs(), event_cursor)? {
            Target::Dao(input) | Target::Project(None, input) => (dao, input),
            Target::Project(Some(id), input) => {
                let resolved = chain
                    .dry_run(origin, dao, routing::resolve_project(id))
                    .await?
                    .outcome
                    .map_err(|_| Error::Invariant("resolve_project did not run"))?;
                if resolved.reverted {
                    // The Dao's refusal is this command's outcome.
                    let transition = routing::transition(true, &resolved.data)?;
                    let receipt = state.shell.complete(pending, transition)?;
                    self.commit(state, false).await?;
                    return Ok(receipt);
                }
                (routing::resolved(&resolved.data)?, input)
            }
        };

        let run = chain.dry_run(origin, contract, input.clone()).await?;
        let value = run.outcome.map_err(|error| {
            // Out of gas or a trap: nothing to record, the caller retries.
            tracing::error!(error, "the contract did not run");
            Error::Invariant("the contract did not run")
        })?;
        let transition = match routing::unsupported_planning(&command, value.reverted, &value.data)?
        {
            Some(code) => Err(code),
            None => routing::transition(value.reverted, &value.data)?,
        };
        let submitted = transition.is_ok();
        let operation = pending.call().operation_id;
        let attempt = pending.clone();
        let mut receipt = state.shell.complete(pending, transition)?;
        if submitted {
            let authority = Authority::Session {
                account: origin,
                key,
            };
            // An error here leaves it unknown whether the call was included:
            // nothing is recorded and the caller asks again.
            // The call may lock a tenth more deposit than its dry-run did,
            // and one storage entry more: it lands a block later.
            let deposit = run
                .storage_deposit
                .saturating_add(run.storage_deposit / 10)
                .saturating_add(DEPOSIT_PER_ENTRY);
            let included = chain
                .call_within(
                    authority,
                    contract,
                    input,
                    run.gas_required.with_margin(),
                    deposit,
                )
                .await?;
            if let Some(failure) = &included.failure {
                // Included and failed, its fee paid: that is its outcome.
                tracing::error!(failure, %operation,
                    "a call failed on the chain after a clean dry-run");
                state = (*original).clone();
                receipt = state.shell.complete(attempt, Err(FAILED_ON_CHAIN))?;
            }
            if self.chain_ms().await.ok() != Some(block_ms) {
                tracing::error!(%operation, "a call landed at another time than its receipt says");
            }
        }
        self.commit(state, submitted).await?;
        Ok(receipt)
    }

    /// Run a read of one state on the chain thread.
    async fn read<T, F>(&self, read: F) -> transport::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut kreivo::Reader<'_>, &State) -> Result<T, Error> + Send + 'static,
    {
        let state = self.current().map_err(transport)?;
        self.0
            .chain
            .mock()
            .read(move |reader| read(reader, &state))
            .await
            .map_err(Error::from)
            .and_then(|result| result)
            .map_err(transport)
    }

    /// The router of the provider's and the wallet's private interfaces, for
    /// requests made in this process with [`Provider::token`].
    ///
    /// # Errors
    /// Does not fail in practice: the token is always long enough.
    pub fn router(&self) -> Result<axum::Router, Error> {
        if let Some(router) = self.0.router.get() {
            return Ok(router.clone());
        }
        let router = transport::router(self.clone(), &self.0.token)
            .map_err(Error::Provider)?
            .merge(crate::wallet::router(self.clone()));
        Ok(self.0.router.get_or_init(|| router).clone())
    }

    /// The bearer token [`Provider::router`] expects.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.0.token
    }
}

/// What a refusal to take a device-signed transaction means: the chain does
/// not verify the credential or the request is too old to submit, which a
/// new attempt answers; or something else is wrong.
fn refused(error: kreivo::Error) -> Error {
    match error {
        kreivo::Error::Rejected(reason)
            if ["BadSigner", "Stale", "AncientBirthBlock"]
                .iter()
                .any(|cause| reason.contains(cause)) =>
        {
            Error::Provider(transport::Error::domain("invalid_credentials"))
        }
        error => error.into(),
    }
}

/// What it means that the chain included a device's transaction and failed
/// it. The chain took the credential, so it is not a wrong device.
fn dispatch_failed(failure: &str, what: &'static str) -> Error {
    tracing::warn!(failure, what);
    if failure == "Pass::MaxSessionsExceeded" {
        capacity_exceeded()
    } else {
        Error::Invariant(what)
    }
}

fn capacity_exceeded() -> Error {
    Error::Provider(transport::Error {
        code: "capacity_exceeded",
        status: axum::http::StatusCode::TOO_MANY_REQUESTS,
    })
}

fn transport(error: Error) -> transport::Error {
    match error {
        Error::Provider(error) => error,
        Error::Shell(error) => error.into(),
        error => {
            tracing::error!(%error, "provider unavailable");
            transport::Error::unavailable()
        }
    }
}

impl ProviderApi for Provider {
    async fn info(&self) -> transport::Result<ProviderInfo> {
        Ok(self.current().map_err(transport)?.shell.info.clone())
    }

    async fn nonce(&self, account: Account) -> transport::Result<AccountNonce> {
        Ok(AccountNonce {
            account,
            nonce: self.current().map_err(transport)?.shell.nonce(account),
        })
    }

    async fn snapshot(&self) -> transport::Result<CoreSnapshot> {
        let head = self
            .0
            .chain
            .info()
            .await
            .map_err(|error| transport(error.into()))?
            .head
            .number;
        if let Ok(cached) = self.0.snapshot.lock()
            && let Some((at, snapshot)) = cached.as_ref()
            && *at == head
        {
            return Ok(snapshot.clone());
        }
        let snapshot = self.read(reads::snapshot).await?;
        if let Ok(mut cached) = self.0.snapshot.lock() {
            *cached = Some((head, snapshot.clone()));
        }
        Ok(snapshot)
    }

    async fn task_state(&self) -> transport::Result<protocol::provider_core::TaskState> {
        self.read(reads::task_state).await
    }

    async fn receipt(&self, operation: OperationId) -> transport::Result<Option<OperationReceipt>> {
        Ok(self
            .current()
            .map_err(transport)?
            .shell
            .receipts
            .get(&operation)
            .map(|record| record.receipt.clone()))
    }

    async fn task_allocation(
        &self,
        operation: OperationId,
    ) -> transport::Result<Option<protocol::provider_core::TaskAllocation>> {
        self.read(move |reader, state| reads::task_allocation(reader, state, operation))
            .await
    }

    async fn events(&self, after: u64, limit: usize) -> transport::Result<ProviderEvents> {
        if !(1..=1000).contains(&limit) {
            return Err(transport::Error::bad("invalid_event_limit"));
        }
        let state = self.current().map_err(transport)?;
        let events: Vec<_> = state
            .shell
            .events
            .iter()
            .filter(|event| event.cursor > after)
            .take(limit)
            .cloned()
            .collect();
        Ok(ProviderEvents {
            provider_instance_id: state.shell.info.provider_instance_id,
            next_cursor: events.last().map_or(after, |event| event.cursor),
            events,
        })
    }

    async fn execute(
        &self,
        call: SignedContractCallV1,
        _now: UnixSeconds,
    ) -> transport::Result<OperationReceipt> {
        // The time of a call is its block's, not the wall clock's. The work
        // runs to its end even if whoever asked stops waiting: a call that
        // was sent must get its receipt.
        let provider = self.clone();
        tokio::spawn(async move { provider.run(call).await })
            .await
            .map_err(|_| transport::Error::unavailable())?
            .map_err(transport)
    }

    async fn lifecycle_reasons(
        &self,
        after: u64,
        limit: usize,
    ) -> transport::Result<protocol::provider_core::ReasonPageV1> {
        self.read(move |reader, state| reads::lifecycle_reasons(reader, state, after, limit))
            .await
    }

    async fn skill_requests(
        &self,
        origin: Account,
    ) -> transport::Result<Vec<protocol::provider_core::SkillRequestCore>> {
        self.read(move |reader, state| reads::skill_requests(reader, state, origin))
            .await
    }

    async fn dispute(
        &self,
        id: protocol::EntityId,
    ) -> transport::Result<protocol::provider_core::CoreDisputeView> {
        self.read(move |reader, state| reads::dispute(reader, state, id))
            .await
    }

    async fn bramp_deposit(
        &self,
        origin: Account,
        id: protocol::EntityId,
    ) -> transport::Result<protocol::DepositView> {
        self.read(move |reader, state| reads::bramp_deposit(reader, state, origin, id))
            .await
    }

    async fn bramp_withdrawal(
        &self,
        origin: Account,
        id: protocol::EntityId,
    ) -> transport::Result<protocol::WithdrawalView> {
        self.read(move |reader, state| reads::bramp_withdrawal(reader, state, origin, id))
            .await
    }
}
