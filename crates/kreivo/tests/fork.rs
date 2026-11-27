//! The chain client against an in-process fork of Kreivo, with the pinned
//! Dao and Project contracts and pallet-pass accounts.
//!
//! It needs a full-state snapshot of Kreivo (about 25 MB), made with sube's
//! `fork_snapshot` example, so it is ignored by default:
//!
//! ```sh
//! KREIVO_FORK_SNAPSHOT=/path/to/kreivo.snap \
//!   cargo test -p kreivo --features fork --test fork -- --ignored
//! ```
#![cfg(feature = "fork")]

use std::error::Error;

use kreivo::{Authority, Chain, DeviceKey, DryRun, Key, Origin, Receipt, UserId, Weight, message};
use parity_scale_codec::Decode;
use protocol::provider_core::{
    CatalogEntryCore, MetadataCommitmentV1, RegisterWorkerCoreRequest, SkillRequestCore,
    SkillRoleCore, WorkerCore,
};
use protocol::{
    AccountId32, CalendarDefinition, Minutes, OperationId, PayloadHash, Qualifications,
};
use zeroize::Zeroizing;

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

/// One native unit: Kreivo's token has 12 decimals.
const UNIT: u128 = 1_000_000_000_000;
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../services/contract-provider"
);
/// The provider's local budget for the Dao constructor, which needs more
/// proof than a Kreivo block carries.
const CONSTRUCTOR_GAS: Weight = Weight {
    ref_time: 1_000_000_000_000,
    proof_size: 16 * 1024 * 1024,
};
/// What Kreivo allows one normal extrinsic (`System.BlockWeights`, spec 136).
const EXTRINSIC: Weight = Weight {
    ref_time: 466_558_509_666,
    proof_size: 2_446_677,
};
/// Plenty for a small Dao message, and far from a block's capacity.
const SMALL: Weight = Weight {
    ref_time: 5_000_000_000,
    proof_size: 500_000,
};

fn snapshot() -> Outcome<String> {
    Ok(std::env::var("KREIVO_FORK_SNAPSHOT")
        .map_err(|_| "KREIVO_FORK_SNAPSHOT must name a Kreivo snapshot")?)
}

fn key(seed: u8) -> Outcome<Key> {
    Ok(Key::from_seed(&Zeroizing::new([seed; 32]))?)
}

/// The WASM of a `.contract` bundle.
fn code(name: &str) -> Outcome<Vec<u8>> {
    let bundle: serde_json::Value = serde_json::from_slice(&std::fs::read(format!(
        "{FIXTURES}/fixtures/{name}.contract"
    ))?)?;
    let hex = bundle["source"]["wasm"]
        .as_str()
        .and_then(|hex| hex.strip_prefix("0x"))
        .ok_or("no wasm in the bundle")?;
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            Ok(u8::from_str_radix(
                hex.get(at..at + 2).ok_or("odd hex")?,
                16,
            )?)
        })
        .collect()
}

type Catalog = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
);
/// The provider's seed fixture: the catalog, then its workers and requests.
type Seed = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
    Vec<WorkerCore>,
    Vec<SkillRequestCore>,
);

fn catalog() -> Outcome<Catalog> {
    let seed: Seed = serde_json::from_slice(&std::fs::read(format!(
        "{FIXTURES}/tests/fixtures/catalog.json"
    ))?)?;
    Ok((seed.0, seed.1, seed.2))
}

/// The value an ink! message returned: `Result<T, LangError>`.
fn returned<T: Decode>(run: &DryRun) -> Outcome<T> {
    let value = run.outcome.as_ref().map_err(Clone::clone)?;
    if value.reverted {
        return Err(format!("reverted: {:?}", value.data).into());
    }
    Ok(Result::<T, u8>::decode(&mut &value.data[..])?
        .map_err(|code| format!("language error {code}"))?)
}

fn succeeded(receipt: &Receipt) -> Outcome {
    match &receipt.failure {
        None => Ok(()),
        Some(failure) => Err(format!("dispatch failed: {failure}").into()),
    }
}

/// Dry-run a message, check it fits a Kreivo extrinsic, then submit it.
async fn transact(
    chain: &Chain,
    authority: Authority,
    caller: AccountId32,
    contract: AccountId32,
    input: Vec<u8>,
) -> Outcome<Receipt> {
    let run = chain.dry_run(caller, contract, input.clone()).await?;
    let value = run.outcome.as_ref().map_err(Clone::clone)?;
    assert!(!value.reverted, "dry-run reverted: {:?}", value.data);
    let gas = run.gas_required.with_margin();
    assert!(
        gas.ref_time <= EXTRINSIC.ref_time && gas.proof_size <= EXTRINSIC.proof_size,
        "{gas:?} does not fit a Kreivo extrinsic"
    );
    let receipt = chain.call(authority, contract, input, gas).await?;
    succeeded(&receipt)?;
    Ok(receipt)
}

fn has(receipt: &Receipt, pallet: &str, variant: &str, field: &str) -> bool {
    receipt.events.iter().any(|event| {
        event.pallet == pallet
            && event.variant == variant
            && event
                .fields
                .as_deref()
                .is_some_and(|fields| fields.contains(field))
    })
}

/// A fork with the contracts deployed by a funded operator.
struct World {
    chain: Chain,
    operator: Key,
    dao: AccountId32,
    /// The fork block's height.
    start: u32,
}

async fn world() -> Outcome<World> {
    let chain = Chain::connect(&format!("fork:{}", snapshot()?)).await?;
    let info = chain.info().await?;
    assert!(info.fork);
    assert_eq!(info.max_session_blocks, 150);

    let operator = key(1)?;
    chain
        .mock()
        .endow(operator.account(), 100_000 * UNIT)
        .await?;
    let dao = chain
        .mock()
        .deploy(
            operator.account(),
            code("dao")?,
            message("new", (operator.account(), operator.account(), catalog()?)),
            b"digesta".to_vec(),
            CONSTRUCTOR_GAS,
        )
        .await?;
    let project_code = chain
        .mock()
        .upload(operator.account(), code("project")?)
        .await?;
    transact(
        &chain,
        Authority::Key(operator.clone()),
        operator.account(),
        dao,
        message("configure_project_code", project_code),
    )
    .await?;
    Ok(World {
        chain,
        operator,
        dao,
        start: info.head.number,
    })
}

/// A user's wallet key, which they register as their pass device.
struct Device(Key);

impl Device {
    fn new(seed: u8) -> Outcome<Self> {
        Ok(Self(key(seed)?))
    }

    fn id(&self) -> AccountId32 {
        self.0.account()
    }

    async fn sign(&self, message: &[u8]) -> Outcome<[u8; 64]> {
        Ok(self.0.sign(message).await?)
    }
}

/// Register `name` with `device`, the operator paying, and fund the account.
async fn enroll(world: &World, name: &str, device: &Device) -> Outcome<AccountId32> {
    let enrollment = world.chain.enrollment(UserId::of(name)).await?;
    let account = enrollment.account;
    let signed = DeviceKey {
        public: device.id(),
        signature: device.sign(&enrollment.message).await?,
    };
    succeeded(
        &world
            .chain
            .register(world.operator.clone(), enrollment, signed)
            .await?,
    )?;
    succeeded(
        &world
            .chain
            .transfer(world.operator.clone(), account, 100 * UNIT)
            .await?,
    )?;
    Ok(account)
}

/// Log `name` in: their device authorizes a fresh session key.
async fn login(world: &World, name: &str, device: &Device, seed: u8) -> Outcome<Authority> {
    let session = key(seed)?;
    let request = world
        .chain
        .session_request(UserId::of(name), device.id(), session.account())
        .await?;
    let signature = device.sign(&request.message).await?;
    let account = request.account;
    let opened = world.chain.open_session(request, signature).await?;
    succeeded(&opened)?;
    assert!(has(&opened, "Pass", "SessionCreated", ""));
    Ok(Authority::Session {
        account,
        key: session,
    })
}

fn registration() -> RegisterWorkerCoreRequest {
    RegisterWorkerCoreRequest {
        content: MetadataCommitmentV1 {
            operation_id: OperationId::from_bytes([9; 16]),
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

async fn worker(
    chain: &Chain,
    dao: AccountId32,
    account: AccountId32,
) -> Outcome<Option<WorkerCore>> {
    let read = chain
        .dry_run(account, dao, message("worker", account))
        .await?;
    returned(&read)
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn dev_operations_build_blocks_that_reads_see() -> Outcome {
    let world = world().await?;
    // endow, deploy and upload each built one; the configuring call another.
    assert_eq!(world.chain.info().await?.head.number, world.start + 4);
    assert!(
        matches!(
            world
                .chain
                .mock()
                .endow(world.operator.account(), UNIT)
                .await,
            Err(kreivo::Error::Invalid(_))
        ),
        "an existing account is not overwritten"
    );
    let read = world
        .chain
        .dry_run(world.operator.account(), world.dao, message("workers", ()))
        .await?;
    assert_eq!(returned::<Vec<WorkerCore>>(&read)?, vec![]);
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn the_chain_verifies_devices() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;

    // Registration: a signature over anything else is not an attestation.
    let enrollment = world.chain.enrollment(UserId::of("alice")).await?;
    let wrong = DeviceKey {
        public: device.id(),
        signature: device.sign(b"something else").await?,
    };
    let refused = world
        .chain
        .register(world.operator.clone(), enrollment, wrong)
        .await?;
    assert_eq!(
        refused.failure.as_deref(),
        Some("Pass::DeviceAttestationInvalid")
    );
    let alice = enroll(&world, "alice", &device).await?;

    // Login: nor does it open a session, and the transaction is not included.
    let request = world
        .chain
        .session_request(UserId::of("alice"), device.id(), key(9)?.account())
        .await?;
    assert_eq!(request.account, alice);
    let forged = world
        .chain
        .open_session(request, device.sign(b"something else").await?)
        .await;
    assert!(
        matches!(forged, Err(kreivo::Error::Rejected(_))),
        "{forged:?}"
    );

    // A key that no device authorized cannot act for the account.
    let stranger = Authority::Session {
        account: alice,
        key: key(10)?,
    };
    let input = message("set_worker_mode", protocol::WorkerMode::Worker);
    let refused = world.chain.call(stranger, world.dao, input, SMALL).await;
    assert!(
        matches!(refused, Err(kreivo::Error::Rejected(_))),
        "{refused:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_session_key_calls_contracts_as_the_pass_account() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;
    let alice = enroll(&world, "alice", &device).await?;
    let as_alice = login(&world, "alice", &device, 9).await?;
    let Authority::Session { key: session, .. } = &as_alice else {
        return Err("login returns a session".into());
    };
    let session = session.account().to_string();

    let receipt = transact(
        &world.chain,
        as_alice,
        alice,
        world.dao,
        message("register_worker", (registration(), [1_u8; 16])),
    )
    .await?;
    // The pass account is the contract's caller and pays; the key holds nothing.
    let alice_hex = alice.to_string();
    assert!(has(&receipt, "Contracts", "Called", &alice_hex));
    let paid = |who| has(&receipt, "TransactionPayment", "TransactionFeePaid", who);
    assert!(paid(&alice_hex) && !paid(&session));

    let registered = worker(&world.chain, world.dao, alice).await?;
    assert_eq!(registered.map(|worker| worker.account), Some(alice));
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_pass_account_registered_first_deploys_and_operates_the_dao() -> Outcome {
    let chain = Chain::connect(&format!("fork:{}", snapshot()?)).await?;
    let start = chain.info().await?.head.number;
    let registrar = key(1)?;
    chain
        .mock()
        .endow(registrar.account(), 100_000 * UNIT)
        .await?;

    // An address that already holds a balance cannot become a pass account.
    let device = Device::new(7)?;
    let early = chain.enrollment(UserId::of("funded-first")).await?;
    chain.mock().endow(early.account, UNIT).await?;
    let signed = DeviceKey {
        public: device.id(),
        signature: device.sign(&early.message).await?,
    };
    let refused = chain.register(registrar.clone(), early, signed).await?;
    assert_eq!(
        refused.failure.as_deref(),
        Some("Pass::AccountAlreadyRegistered")
    );

    // So the administrator is registered, then funded, then deploys.
    let enrollment = chain.enrollment(UserId::of("admin")).await?;
    let admin = enrollment.account;
    let signed = DeviceKey {
        public: device.id(),
        signature: device.sign(&enrollment.message).await?,
    };
    succeeded(
        &chain
            .register(registrar.clone(), enrollment, signed)
            .await?,
    )?;
    // More than Kreivo's issuance before the fork: endowments count in it.
    succeeded(
        &chain
            .transfer(registrar.clone(), admin, 10_000 * UNIT)
            .await?,
    )?;
    let dao = chain
        .mock()
        .deploy(
            admin,
            code("dao")?,
            message("new", (admin, admin, catalog()?)),
            b"digesta".to_vec(),
            CONSTRUCTOR_GAS,
        )
        .await?;
    let project_code = chain.mock().upload(admin, code("project")?).await?;
    let world = World {
        chain,
        operator: registrar,
        dao,
        start,
    };

    // Only the operator sets the Project code: the registrar is refused, and
    // the administrator's session key is not.
    let input = message("configure_project_code", project_code);
    let as_registrar = world
        .chain
        .dry_run(world.operator.account(), dao, input.clone())
        .await?;
    let refused = as_registrar.outcome.as_ref().map_err(Clone::clone)?;
    assert!(refused.reverted, "a stranger configured the Dao");
    let as_admin = login(&world, "admin", &device, 9).await?;
    let receipt = transact(&world.chain, as_admin, admin, dao, input).await?;
    assert!(has(&receipt, "Contracts", "Called", &admin.to_string()));
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn the_fork_is_read_in_place_and_follows_the_clock() -> Outcome {
    let world = world().await?;
    let dao = world.dao;
    let operator = world.operator.account();

    // A message and the contract's own storage, read on the chain thread.
    let (workers, root, before) = world
        .chain
        .mock()
        .read(move |reader| {
            let workers = reader.query(operator, dao, &message("workers", ()));
            // ink! keeps the root struct under the four zero bytes of its key.
            let root = reader.contract_storage(dao, &[0, 0, 0, 0]);
            (workers, root, reader.timestamp())
        })
        .await?;
    assert!(!workers?.reverted);
    assert!(root?.is_some(), "the Dao stores its root");
    assert!(before? > 1_700_000_000_000);

    // An hour later by the wall clock.
    for hours in [1_u64, 10] {
        let before = world
            .chain
            .mock()
            .read(|reader| reader.timestamp())
            .await??;
        let head = world.chain.info().await?.head.number;
        world
            .chain
            .mock()
            .follow_clock(before + hours * 3_600_000)
            .await?;
        let after = world
            .chain
            .mock()
            .read(|reader| reader.timestamp())
            .await??;
        // The fork lands on the relay block that holds the asked time.
        assert_eq!(after, before + hours * 3_600_000);
        assert!(world.chain.info().await?.head.number > head);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_device_is_replaced_in_one_transaction() -> Outcome {
    let world = world().await?;
    let old = Device::new(7)?;
    let new = Device::new(8)?;
    let alice = enroll(&world, "alice", &old).await?;

    // The new device signs an enrollment, the old one the replacement.
    let enrollment = world.chain.enrollment(UserId::of("alice")).await?;
    let signed = DeviceKey {
        public: new.id(),
        signature: new.sign(&enrollment.message).await?,
    };
    let request = world
        .chain
        .replacement_request(old.id(), enrollment, signed)
        .await?;
    assert_eq!(request.account, alice);
    let signature = old.sign(&request.message).await?;
    let replaced = world.chain.open_session(request, signature).await?;
    succeeded(&replaced)?;
    assert!(has(&replaced, "Pass", "DeviceRemoved", ""));

    // The new device opens sessions; the old one no longer does.
    login(&world, "alice", &new, 9).await?;
    let request = world
        .chain
        .session_request(UserId::of("alice"), old.id(), key(10)?.account())
        .await?;
    let signature = old.sign(&request.message).await?;
    assert!(world.chain.open_session(request, signature).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_kept_session_lasts_until_it_is_ended() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;
    let alice = enroll(&world, "alice", &device).await?;
    let as_alice = login(&world, "alice", &device, 9).await?;
    let Authority::Session { key: session, .. } = &as_alice else {
        return Err("login returns a session".into());
    };
    let session = session.account();
    let calendar = || {
        message(
            "set_calendar",
            CalendarDefinition {
                default_weekly_minutes: Minutes::new(300),
                overrides: vec![],
            },
        )
    };
    transact(
        &world.chain,
        as_alice.clone(),
        alice,
        world.dao,
        message("register_worker", (registration(), [1_u8; 16])),
    )
    .await?;

    // Kept, it outlives the relay blocks of a session.
    world.chain.mock().keep_session(session).await?;
    world.chain.mock().advance(200).await?;
    transact(&world.chain, as_alice.clone(), alice, world.dao, calendar()).await?;

    // Ended, the chain refuses what its key signs.
    world.chain.mock().end_session(session).await?;
    let run = world.chain.dry_run(alice, world.dao, calendar()).await?;
    let refused = world
        .chain
        .call(
            as_alice,
            world.dao,
            calendar(),
            run.gas_required.with_margin(),
        )
        .await;
    assert!(
        matches!(refused, Err(kreivo::Error::Rejected(_))),
        "{refused:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_call_locks_no_more_deposit_than_its_limit() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;
    let alice = enroll(&world, "alice", &device).await?;
    let as_alice = login(&world, "alice", &device, 9).await?;
    let input = message("register_worker", (registration(), [1_u8; 16]));
    let run = world.chain.dry_run(alice, world.dao, input.clone()).await?;
    assert!(run.storage_deposit > 0, "registering stores something");
    let gas = run.gas_required.with_margin();

    // Under what the call needs, the chain includes it and fails it.
    let short = world
        .chain
        .call_within(
            as_alice.clone(),
            world.dao,
            input.clone(),
            gas,
            run.storage_deposit - 1,
        )
        .await?;
    assert!(short.failure.is_some(), "the call fit a smaller deposit");
    assert_eq!(worker(&world.chain, world.dao, alice).await?, None);

    // With what the dry-run reported, it succeeds.
    let exact = world
        .chain
        .call_within(as_alice, world.dao, input, gas, run.storage_deposit)
        .await?;
    succeeded(&exact)?;
    assert!(worker(&world.chain, world.dao, alice).await?.is_some());
    Ok(())
}

/// The account of the community with this ID: the pallet's ID, then the
/// community's.
fn community_account(id: u16) -> AccountId32 {
    let mut bytes = [0; 32];
    bytes[..4].copy_from_slice(b"modl");
    bytes[4..12].copy_from_slice(b"kv/cmtys");
    bytes[12..14].copy_from_slice(&id.to_le_bytes());
    AccountId32::from_bytes(bytes)
}

/// What governance and a community do on Kreivo, on the fork: a community
/// with funds, a usage-subscription offer, the community's subscription to
/// it and two memberships to give. Returns the community's account.
async fn subscribed_community(world: &World, community: u16) -> Outcome<AccountId32> {
    let mock = world.chain.mock();
    let account = community_account(community);
    mock.dispatch(
        Origin::Root,
        "communitiesManager/register".into(),
        // The name is "Digesta".
        format!(
            "(community_id:{community};name:0x44696765737461;first_admin:MultiAddress::Id({});\
             maybe_decision_method:Option::None;maybe_track_info:Option::None)",
            world.operator.account(),
        ),
    )
    .await?;
    mock.dispatch(
        Origin::Root,
        "balances/force_set_balance".into(),
        format!(
            "(who:MultiAddress::Id({account});new_free:{})",
            1_000 * UNIT
        ),
    )
    .await?;
    mock.dispatch(
        Origin::Root,
        "usageSubscription/publish_offer".into(),
        format!(
            "(kind:OfferKind::Standard;terms:(allowance:(ref_time:1000000000000;proof_size:10000000);\
             usage_period:600;price:(asset:FungibleAssetLocation::Here(0);amount:{});\
             billing_period:432000;term:Option::None;min_commitment:Option::None;grace:43200))",
            UNIT / 100
        ),
    )
    .await?;
    mock.dispatch(
        Origin::Signed(account),
        "usageSubscription/subscribe".into(),
        "(offer:0;converts_into:Option::None)".into(),
    )
    .await?;
    mock.dispatch(
        Origin::Signed(account),
        "communitiesManager/issue_memberships".into(),
        "(amount:2)".into(),
    )
    .await?;
    Ok(account)
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_member_without_balance_pays_fees_from_its_community() -> Outcome {
    let world = world().await?;
    let mock = world.chain.mock();
    let community = subscribed_community(&world, 4242).await?;

    // A pass account with nothing: registered, not funded.
    let device = Device::new(7)?;
    let enrollment = world.chain.enrollment(UserId::of("alice")).await?;
    let alice = enrollment.account;
    let signed = DeviceKey {
        public: device.id(),
        signature: device.sign(&enrollment.message).await?,
    };
    succeeded(
        &world
            .chain
            .register(world.operator.clone(), enrollment, signed)
            .await?,
    )?;
    assert_eq!(mock.free_balance(alice).await?, 0);

    // Outside the community it cannot even open a session.
    let request = world
        .chain
        .session_request(UserId::of("alice"), device.id(), key(9)?.account())
        .await?;
    let signature = device.sign(&request.message).await?;
    let unpaid = world.chain.open_session(request, signature).await;
    assert!(
        matches!(&unpaid, Err(kreivo::Error::Rejected(reason)) if reason.contains("Payment")),
        "{unpaid:?}"
    );

    // A member opens it, and the community's pool pays.
    let admit = |account: AccountId32| {
        mock.dispatch(
            Origin::Signed(community),
            "communities/add_member".into(),
            format!("(who:MultiAddress::Id({account}))"),
        )
    };
    admit(alice).await?;
    // The community issued two memberships: a third member finds none, and
    // the failed call is named.
    admit(key(11)?.account()).await?;
    let full = admit(key(12)?.account()).await;
    assert!(
        matches!(&full, Err(kreivo::Error::Rejected(reason)) if reason.ends_with("Communities::CommunityAtCapacity")),
        "{full:?}"
    );
    let session = key(10)?;
    let request = world
        .chain
        .session_request(UserId::of("alice"), device.id(), session.account())
        .await?;
    let signature = device.sign(&request.message).await?;
    let opened = world.chain.open_session(request, signature).await?;
    succeeded(&opened)?;
    assert!(has(&opened, "UsageSubscription", "UsageCharged", ""));
    assert!(!has(
        &opened,
        "TransactionPayment",
        "TransactionFeePaid",
        ""
    ));
    assert_eq!(mock.free_balance(alice).await?, 0);

    // Its session key calls a contract on the pool too, but what the call
    // stores needs a deposit, and the pool does not pay deposits.
    let as_alice = Authority::Session {
        account: alice,
        key: session,
    };
    let input = message("register_worker", (registration(), [1_u8; 16]));
    let broke = world.chain.dry_run(alice, world.dao, input.clone()).await?;
    assert!(broke.outcome.is_err(), "{:?}", broke.outcome);

    // With a balance for the deposit, the call stores what it needs, and
    // the fee still comes from the pool: only the deposit leaves the balance.
    let balance = 10 * UNIT;
    succeeded(
        &world
            .chain
            .transfer(world.operator.clone(), alice, balance)
            .await?,
    )?;
    let run = world.chain.dry_run(alice, world.dao, input.clone()).await?;
    assert!(run.storage_deposit > 0);
    let stored = world
        .chain
        .call_within(
            as_alice,
            world.dao,
            input,
            run.gas_required.with_margin(),
            run.storage_deposit,
        )
        .await?;
    succeeded(&stored)?;
    assert!(has(&stored, "UsageSubscription", "UsageCharged", ""));
    assert!(!has(
        &stored,
        "TransactionPayment",
        "TransactionFeePaid",
        ""
    ));
    assert_eq!(
        mock.free_balance(alice).await?,
        balance - run.storage_deposit
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_session_ends_after_its_relay_blocks() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;
    enroll(&world, "alice", &device).await?;
    let as_alice = login(&world, "alice", &device, 9).await?;
    let lifetime = world.chain.info().await?.max_session_blocks;
    let mode = || message("set_worker_mode", protocol::WorkerMode::Worker);
    let call = |authority: Authority| world.chain.call(authority, world.dao, mode(), SMALL);

    // Still open halfway: the chain takes the transaction, though the Dao
    // turns down a caller who is not a worker.
    world.chain.mock().advance(lifetime / 2).await?;
    let included = call(as_alice.clone()).await?;
    assert_eq!(
        included.failure.as_deref(),
        Some("Contracts::ContractReverted")
    );

    // Past its end the chain no longer knows the key: its account would have
    // to pay for itself, and holds nothing.
    world.chain.mock().advance(lifetime).await?;
    let expired = call(as_alice).await;
    assert!(
        matches!(&expired, Err(kreivo::Error::Rejected(reason)) if reason.contains("Payment")),
        "{expired:?}"
    );

    // A new login opens another one.
    let again = login(&world, "alice", &device, 11).await?;
    assert!(call(again).await?.failure.is_some());
    Ok(())
}

#[tokio::test]
#[ignore = "needs KREIVO_FORK_SNAPSHOT"]
async fn a_saved_fork_resumes_with_its_state() -> Outcome {
    let world = world().await?;
    let device = Device::new(7)?;
    let alice = enroll(&world, "alice", &device).await?;
    let as_alice = login(&world, "alice", &device, 9).await?;
    transact(
        &world.chain,
        as_alice.clone(),
        alice,
        world.dao,
        message("register_worker", (registration(), [1_u8; 16])),
    )
    .await?;

    let saved = std::env::temp_dir().join(format!("kreivo-fork-{}.snap", std::process::id()));
    world.chain.mock().save(saved.clone()).await?;
    let head = world.chain.info().await?.head;
    let dao = world.dao;
    drop(world);
    let resumed = Chain::connect(&format!("fork:{}", saved.display())).await;
    std::fs::remove_file(&saved)?;
    let resumed = resumed?;

    // The contracts, the worker and the still-open session are all there.
    assert_eq!(resumed.info().await?.head, head);
    assert!(worker(&resumed, dao, alice).await?.is_some());
    let calendar = CalendarDefinition {
        default_weekly_minutes: Minutes::new(900),
        overrides: vec![],
    };
    transact(
        &resumed,
        as_alice,
        alice,
        dao,
        message("set_calendar", calendar),
    )
    .await?;
    let updated = worker(&resumed, dao, alice).await?.ok_or("no worker")?;
    assert_eq!(
        updated.calendar.definition.default_weekly_minutes,
        Minutes::new(900)
    );
    Ok(())
}

#[tokio::test]
async fn a_missing_snapshot_fails_to_connect() {
    let connected = Chain::connect("fork:/nonexistent/kreivo.snap").await;
    assert!(matches!(connected, Err(kreivo::Error::Unavailable(_))));
}
