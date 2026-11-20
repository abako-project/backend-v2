# SPEC-0016: Kreivo as the only backend, through sube, with pallet-pass identity

Status: DRAFT. Requested by the owner on 2026-11-20, who decided
[six questions](#decisions) the same day. Stage 1 proceeds on those answers.
Stage 2 waits for the [contract changes](#contract-changes) that finding 10
calls for, and with stage 3 for the owner's approval of this note: they change
authentication and the public API and remove services. Nothing here authorizes
publishing or deploying.

When approved this supersedes SPEC-0001 (custodial signing) and the passkey
section of SPEC-0004, and replaces the execution, authority and persistence
parts of SPEC-0013. SPEC-0015's ownership split (API content, contract facts)
and the marketplace rules of SPEC-0003/0005–0008 are unchanged.

## Outcome

One `api` process and PostgreSQL. The API talks to Kreivo through
[sube](https://github.com/virto-network/virto-sdk/tree/main/crates/sube); the
connection string selects the chain:

| `KREIVO_URL` | Chain | Use |
|---|---|---|
| `fork:<snapshot>` | In-process fork of Kreivo (sube-fork), instant blocks | Local development, tests, the mock-backed preview |
| `wss://…` | A Kreivo node | Live |

The fork runs the real Kreivo runtime, so the same extrinsics, pallet-pass
checks, contract limits and fees apply in both. `services/wallet` and
`services/contract-provider` are removed, with their SQLite stores, the private
HTTP interfaces, the service token, the isolated polkadot-sdk workspace and its
advisory exceptions.

Users are pallet-pass accounts. A passkey (WebAuthn) or a wallet key
(SubstrateKey) is registered on chain as a device; the chain, not the API,
verifies every credential. Passwords, Argon2, `webauthn-rs` ceremonies and
custodial seeds go away.

## What the spike established

The spike boots a fork of Kreivo #41144847 (spec 136), registers pass accounts,
deploys the pinned Dao and Project artifacts and drives them through sube.
Measured on a developer laptop, release build, snapshot
`sha256:80653b01472ac975a29c91245b620af39555e368ff4e28aebf85a4f14b3bc45d`:

| Step | Result | Time | ref_time ps / proof bytes |
|---|---|---|---|
| Load snapshot, boot fork, connect | ok | 25 ms + 110 ms | |
| `Pass.register`, SubstrateKey device, operator as registrar | Success | 12 ms | |
| Dao instantiate with the full catalog seed, as a dev operation | ok | 130 ms | 96.2e9 / **8,788,437** |
| `configure_project_code`, V4 signed by the operator | Success | 34 ms | 0.74e9 / 158,738 |
| `register_worker`, V5 extrinsic with `PassAuthenticate` | Success | 45 ms | 3.84e9 / 446,408 |
| `promote_coordinator`, `set_worker_mode` | Success | 36 ms | 1.0e9 / 179,543 |
| `create_project` (Dao instantiates a Project contract) | Success | 62 ms | 4.70e9 / 662,148 |
| `Pass.add_session_key_with_filter`, only `Contracts.call`, by the device | Success | 11 ms | |
| `set_calendar` signed by that session key | Success | 37 ms | 1.08e9 / 189,715 |
| `System.remark` signed by that session key | rejected, `Invalid::Call` | | |
| Read `worker`, `projects`, `Project.view` with `ContractsApi_call` | ok | 24–26 ms | 0.71e9 / 203,919 (`view`) |
| `Pass.register` with a `<Bytes>`-wrapped wallet signature | Failed, `DeviceAttestationInvalid` | | |
| `Contracts.upload_code`, `instantiate`, `instantiate_with_code` by an account | Failed, `BadOrigin` | | |

Times are submission to finality on the fork and include block building.

Kreivo builds three blocks per core. A normal extrinsic may declare 0.467 s of
ref_time and **2,446,677 bytes of proof** in any of them
(`System.BlockWeights`). The first block of a core may instead give the whole
core to one transaction, up to about 1.4 s and **7,340,032 bytes**; the
collator then builds no other block on that core. Nothing larger is ever
valid. Proof size is the limit everywhere: no operation measured used more
than 17% of the ref_time.

Findings that shape the design:

1. **It works.** The pass account is the contract caller and pays the fee and
   the storage deposit, whether the device or a session key authorizes the
   call. The session account needs no funds. Contract-to-contract instantiation
   and direct client calls to a Project work under Kreivo's limits.
2. **Kreivo forbids direct deployment.** Uploading and instantiating need a
   community origin through `ContractsStore`; plain accounts get `BadOrigin`. A
   fork needs a dev operation that keeps the storage changes of a runtime API
   call (`ContractsApi_instantiate`, `ContractsApi_upload_code`). sube-fork
   lacked it; see decision 1.
3. **The Dao constructor does not fit a Kreivo transaction** with the full
   seed: 8.8 MB of proof, above even a whole core. The fork's dev operation is
   not bound by it; a live deployment must seed the catalog in separate
   transactions.
4. **The contracts emit no events.** Effects (kind, entity ID, recipients,
   allocated task ID) are message return values, visible to a dry-run but not
   to a submitted extrinsic. The chain reports only `Contracts.Called` and
   `Contracts.Instantiated`.
5. **Sessions last 15 minutes at most**, ten per account:
   `MaxSessionDuration` is 150 blocks of the relay chain, pallet-pass's clock on
   Kreivo. The runtime's scheduler removes the session, after which the chain
   treats its key as an ordinary account with no funds (`Invalid::Payment`).
   The chain enforces the session's call filter.
6. **Callers need native balance.** Fees could be paid in an asset
   (`ChargeAssetTxPayment`, not tried), but contract storage deposits are
   native: 0.067 KSM for `register_worker`, 0.047 for `create_project`. A new
   pass account holds nothing. Any signed account may register one and pays.
7. **Assets.** Kreivo has `Here(100..103)` (COPM, Bloque USDC/EURC/BTC) and no
   `Here(1)`, the provider's fixture asset. The payment asset must be
   configuration. A fork creates the mock asset with one storage write of
   `Assets.Asset`, after which the operator mints it with an ordinary extrinsic.
8. **Browser wallet extensions cannot register today.** They sign `signRaw`
   payloads as `<Bytes>…</Bytes>`; Kreivo's substrate-keys authenticator verifies
   the bare message and rejects the attestation. A key that signs the bare
   message (libwallet, sube's CLI) works. See decision 4 for the fix.
9. sube is single-threaded (`Rc`, smol). It runs on one dedicated thread behind
   a bounded channel, like the provider's worker today.

### The marketplace on the fork

A second run drove the whole marketplace with 13 pass accounts and the
operator: 193 contract extrinsics, with escrow and payments through Kreivo's
asset extension. Money is conserved: of 20,000 units funded, the final balances
of client, coordinator and workers sum to 20,000, and escrow stays held
through a dispute.

| Operation, 12 workers | Proof bytes | Fits |
|---|---|---|
| One milestone, one requirement, creation to evaluations (18 messages) | at most 1,808,604 | any block |
| `create_proposal`, four milestones, 14 requirements | 4,684,555 | a whole core |
| `approve_execution`, one milestone, one requirement | 1,650,133 | any block |
| `approve_execution`, two milestones, teams of 5 and 3 | 6,388,421 | a whole core |
| `approve_execution`, three milestones, teams 5/3/4 | 8,928,494 | **never** |
| `approve_execution`, four milestones, teams 5/3/4/2 | 10,768,309 | **never** |
| `submit_milestone`, `accept_milestone`, `evaluate_project` of that project | 2.4–3.6 MB | a whole core, all but one |

10. **The marketplace does not fit Kreivo as the contracts stand.** The chain
    refuses the approval of a three or four milestone project
    (`Invalid::ExhaustsResources`); four milestones is SPEC-0013's own
    performance scenario. Of the approvals tried, only one milestone with one
    requirement fits an ordinary block. On a fork these calls can only run as
    dev operations, which is no longer the real chain.
11. **Costs grow with the platform.** Messages walk every worker, project and
    deposit. With 22 workers the approval of one five-person milestone no
    longer fits a core; with 92, neither does the smallest approval, and
    `create_project` and `register_worker` need a whole core each.
12. **Reads are close to their limit.** A dry-run without an explicit gas limit
    gets one block: `catalog` of the seed alone uses 95% of it, and `workers`
    fails between 22 and 32 workers. Neither is paginated.
13. **Deposits are the cost.** A four-milestone project costs its client 0.68
    KSM and its coordinator 1.06, nearly all of it storage deposits; a
    registration 0.08 to 0.20; each evaluation 0.02. Fees are under 0.01 per
    call.
14. **What the provider's shell did is open on a chain.** The selection seed
    is a caller argument, so a client calling the Dao directly can choose its
    coordinator and IDs. Reads that check their caller are not private, since a
    dry-run names any origin without a signature. A reverted call is included
    and pays its fee, and its domain error code exists only in a dry-run.

Not yet measured: WebAuthn devices, deployment through a community, real proof
sizes (the fork meters them by weight), and anything against a live node.

Where a fork differs from Kreivo, found on the way:

- It builds every block as the first of a core, so it admits a whole-core
  transaction in any block. Live Kreivo does in one block of three.
- After a jump in time the scheduler serves the skipped relay blocks a few
  dozen per block and refuses transactions meanwhile; the mock builds those
  blocks itself.
- A dry-run without a gas limit gets one block, and then reports a large read
  as a trapped contract. The client always sets one.

The logs are in [`results/`](results/): `market-final.out` has every operation
of the marketplace run, `mlimits.out` the limits and `wallet-test.log` the
wallet signatures. The spike sources sit in `spike/` beside this note,
untracked, with the snapshot: they link GPL crates and are evidence, not backend
code. `main.rs` is the first run, `bin/market.rs` the marketplace,
`bin/mlimits.rs` the limits and `bin/wallet.rs` the wallet signatures.

## Requirements

### Chain access

- A `kreivo` crate owns the sube connection on one thread and exposes `Send`
  handles with bounded queues. It prepares, submits and tracks extrinsics,
  dry-runs and reads contracts with `ContractsApi_call`, and follows finalized
  blocks. No other code holds a sube handle.
- Fork support is a cargo feature. The mock build used by Compose, tests and
  the preview enables it; the live image does not, and links neither smoldot
  nor the dev operations.
- Startup checks the genesis hash, that the configured Dao exists with the pinned
  code hash, and that the runtime has pallet-pass, pallet-contracts and the
  asset extension. A mismatch fails startup.
- In fork mode a first start deploys and seeds the contracts, funds the
  operator and mints the mock payment asset. Fork state persists with
  PostgreSQL; a store that does not match the fork fails startup instead of being
  reset.
- ABI routing (`ProviderCommand` to contract and input) moves out of the
  provider into shared code without reproducing business rules.

### Identity and sessions

- A principal is a pass account. Its user ID is the hash of the normalized
  username; display data stays in PostgreSQL keyed by account.
- Registration: the browser's device attests a challenge bound to a recent block;
  the operator submits `Pass.register`. Login: the device authenticates a
  `Pass.add_session_key_with_filter` extrinsic. Finalized success is the
  authentication; the API verifies no device signature itself.
- The API session keeps its HttpOnly cookie and CSRF token. It cannot outlive
  the on-chain session; renewing needs a fresh device assertion.
- Session keys are limited to `Contracts.call` and are never `Admin`.

### Operations

- Business writes keep the public operation reference and polling. The chain
  nonce, mortality, finality and events replace the envelope nonce, provider
  generation, `SignedContractCallV1` and SQLite receipts.
- The exact signed extrinsic is stored before submission. After an
  interruption the same bytes are resubmitted or their inclusion is found;
  an operation is `OutcomeUnknown` until that resolves, and never replaced by a
  new extrinsic while it could still be included.
- A dry-run precedes submission to size gas and reject domain errors early. A
  failed dispatch is a rejected operation with its fee paid.
- Exact money rules are unchanged: checked integers, decimal strings.

### Reads and notifications

- Reads use contract messages at a finalized block, not pinned storage layouts.
  Enumerations must be bounded before data outgrows a dry-run.
- Notifications derive from finalized chain data. Until the contracts emit
  events, only operations submitted by this API produce them.

## Contract changes

What makes the calls heavy, read in the pinned source
(`abako-dev/contracts` at `a0532843`) and measured in findings 10 and 11. The
proof charged follows the number of storage items a message touches, and the
code of every contract it calls.

1. **Check ID uniqueness by key, not by walking everything.**
   `Dao::validate_public_ids` loads every worker, every project's layout and
   submissions, every deposit and every dispute to look for a duplicate. It runs
   in `register_worker`, `create_project`, the deposit and withdrawal requests
   and the three calls a Project makes back into the Dao. One set keyed by ID
   answers the same question with one read. With the next item, this is why
   costs grow with the platform.
2. **Select without loading the directory.** `coordinator_selection` loads
   every worker; `assign_team` loads every holder of a requirement's first
   skill, once per requirement. Keep the eligible accounts and their capacity
   per week in keyed entries, or let the caller name the candidates and have
   the contract verify them.
3. **Split the approval.** `approve_execution` assigns every milestone's team,
   reserves every calendar and takes the whole escrow in one transaction: 1.65
   MB for one requirement, about 0.8 MB more for each further one. Approve the
   proposal and escrow once, then assign and reserve one milestone per
   transaction.
4. **Load only what a message changes.** A Project keeps its parts in separate
   keyed entries but reads and writes all of them on every message, so every
   task adds 20 kB to each later call. Read and write the entries the message
   needs.
5. **Page the lists.** `catalog`, `workers`, `projects` and `Project.view`
   return everything. Give them a start and a count.
6. **Seed the catalog after deployment**, in transactions, not in the
   constructor.
7. **Emit events** for each effect, with its entity, recipients and allocated
   IDs, and for each domain error that today exists only as return data.
8. **Stop trusting the caller for what the shell supplied**: derive selection
   seeds and IDs in the contract, and do not rely on the caller's identity to
   keep a read private.

The acceptance line for these is the first bullet under [acceptance](#acceptance),
measured again with `spike/src/bin/market.rs`.

## Stages

1. **Chain client and mock.** Wallet and provider untouched. Done: the
   marketplace measured on the fork, and the [`kreivo` crate](../../crates/kreivo/README.md)
   with contract calls, key devices, sessions and the fork's dev operations.
   Left: the mock's bootstrap (asset, seed, operator), ABI routing shared with
   the provider, following finalized blocks, and WebAuthn devices.
2. **API on the chain.** Reads, operations, pass registration and login, new
   auth routes, `poc-e2e.py` and seed scripts rewritten. Fresh data only, as
   SPEC-0015 already allows. Waits for the contract changes (decision 5).
3. **Removal.** Delete `services/wallet` and `services/contract-provider`, their
   images, Compose services, secrets, policy files and documents.
4. **Live readiness**, separately approved: contract events, split catalog
   seeding, community deployment, funding of fees and deposits, recovery against
   a real node, browser WebAuthn and wallet signing.

Each stage ends with the independent review SPEC-0013 requires.

## Acceptance

- Every operation of the marketplace scenario fits a normal Kreivo extrinsic
  (0.467 s, 2,446,677 bytes of proof) on the fork, or is listed with its
  measured weight as a contracts blocker. The list is in finding 10 and
  [`results/market-final.out`](results/market-final.out).
- The public gateway flow passes in fork mode from registration to settlement
  with a passkey device.
- Invalid credential, expired session, call outside the session filter, replay,
  restart mid-submission and a fork that does not match PostgreSQL are exercised.
- `scripts/verify.sh app` runs the chain tests against a pinned snapshot.
- The live image contains no fork code.

## Decisions

Decided by the owner on 2026-11-20:

1. **sube-fork is extended.** `Fork::call_mut` keeps the storage changes of a
   runtime API call and `Fork::snapshot` saves a fork so it persists with
   PostgreSQL. Both were merged in
   [virto-sdk#235](https://github.com/virto-network/virto-sdk/pull/235), with a
   fix for sube reading a stale finalized head; the backend pins that revision
   of `main`. The mock asset needs nothing more (finding 7).
2. **The source stays Apache-2.0 and takes the GPL crates.** `deny.toml` gets
   exceptions for the named virto-sdk crates and their git source. A built API
   binary or image links them and is distributed under GPL-3.0-or-later.
3. **The API holds a session key per login**, in memory, limited to
   `Contracts.call` and expiring on chain within 15 minutes. Business routes keep
   their shape. Password and `webauthn-rs` routes are removed, registration and
   login become two-step device ceremonies, and `accountId` is the pass account;
   both frontends change. A browser-held key remains the live target.
4. **Wallet login is in the first cut.** Kreivo's substrate-keys authenticator
   learns the `<Bytes>` wrapping. The change was merged in
   [pass-authenticators#36](https://github.com/virto-network/pass-authenticators/pull/36).
   On a fork running a spec-136 runtime built with it, wrapped signatures
   register a device and
   authenticate extrinsics, bare ones still do, and other wrappings are refused.
   Going live needs a Kreivo runtime upgrade, with two things settled first:
   - A wrapped or invalid signature now costs two verifications where the
     runtime's weights charge one; the benchmarks must sign the wrapped form.
   - Extensions wrap raw data so that approving it can never carry on-chain
     authority. Accepting the wrapping means a page that gets a user to approve
     68 opaque bytes holds a credential for whatever call they commit to. The
     wallet shows only hex. See decision 6.

Decided by the owner on 2026-11-20, after the measurements:

5. **The contracts change first.** The scenario must fit ordinary blocks
   before the fork replaces the provider, which stays until then. The mock does
   not commit what Kreivo refuses. See [contract changes](#contract-changes).
6. **The wallet patch proceeds.** It was merged as it is; ways to reduce its
   phishing surface are explored later. Options, strongest first:
   - *Sign a transaction, not bytes.* Let a registered key device authorize the
     pass account by signing an ordinary extrinsic, as a session key already
     does. A wallet then shows the decoded call through `signPayload` and no
     raw signature carries authority. This needs a pallet-pass change and
     makes the wrapping unnecessary.
   - *Sign a sentence.* Have the authenticator verify a fixed, readable
     statement built from the action, authority, block and challenge, in the
     manner of Sign-In with Ethereum, instead of 68 opaque bytes. The wallet
     displays it, and a page cannot make another text verify.
   - *Limit what a wallet can authorize.* Register wallet keys with a device
     filter that allows only opening a non-admin session, and keep a passkey as
     the account's admin device. A phished signature then yields 15 minutes of
     contract calls at most.
   - *Shorten the challenge's life* to a few blocks, so a signature obtained
     ahead of time is useless.

Open:

7. **What the fork adds to the dependency findings.** Through smoldot:
   `constant_time_eq` 0.1.5 under CC0-1.0, which the policy already rejects for
   five other crates pending the owner's decision, and `libsecp256k1`, reported
   unmaintained. Neither is silenced. Both come only with the `fork` feature.

Defaults, unless the owner says otherwise:

- **Operator key.** One key file in the API is the `Pass.register` registrar
  and the Dao's controller for privileged messages, replacing the custody root
  seed. It holds no user funds. A community origin replaces it on a live DAO.
- **Effects without events.** Use the dry-run's effect to route the operation,
  and read values that depend on execution-time state (allocated IDs, recipients)
  back at the finalized block, until the contracts emit events.
- **Snapshot for CI.** A pinned 25 MB artifact with its hash, refreshed after
  Kreivo runtime upgrades, rather than a download from the live node per run.

## Risks

- A session key held by the API can call any contract as the user until it
  expires, including with attached value; the filter restricts the call, not
  its destination.
- A transaction that needs a whole core waits for the first block of one and
  stops the collator building the other two. Several in a row slow the chain
  for everyone.
- Seeds chosen by callers and reads open to any origin (finding 14) need
  contract changes before a public chain; the API cannot enforce them.
- A snapshot contains live state and the live runtime at one block. A runtime
  upgrade can change pallet-pass or contract behavior before the snapshot is
  refreshed.
- Live latency is block time and finality, not milliseconds; login submits a
  transaction, and users need native balance before their first write.
- sube-fork is 0.x and its state roots are not authoritative.
