# SPEC-0016: Kreivo as the only backend, through sube, with pallet-pass identity

Status: DRAFT. Requested by the owner on 2026-11-20, who decided
[six questions](#decisions) the same day. Stage 1 proceeds on those answers.
Stage 2 waits for the [contract changes](#contract-changes) that finding 10
calls for, and with stage 3 for the owner's approval of this note: they change
authentication and the public API and remove services. The first contract
changes, [contracts#23](https://github.com/abako-dev/contracts/pull/23), are
merged and the backend pins them (findings 15 to 17). One approval still needs
a whole core, so stage 2 keeps waiting (open decision 9). Nothing here
authorizes publishing or deploying.

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
deploys the Dao and Project artifacts the backend pinned at the time
(`abako-dev/contracts` at `a0532843`, "the earlier contracts" below) and drives
them through sube.
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

#### After the first contract changes

Findings 10 to 13 describe the earlier contracts and stay as the baseline. The
same run was repeated on 2026-11-25 with
[contracts#23](https://github.com/abako-dev/contracts/pull/23) at `2a74d50`.
It was merged as `aa25bad` with the same code, and the backend pins those
artifacts since [#41](https://github.com/abako-project/backend-v2/pull/41). It
keeps every public message and replaces five of the hooks Project calls on Dao:

| Of 193 transactions | Before | contracts#23 |
|---|---|---|
| Fit an ordinary block with the 10% gas margin the client adds | 150 | 191 |
| Fit an ordinary block without the margin | 161 | 192 |
| Need a whole core | 30 | 1 |
| Refused | 2 | 0 |

| Heaviest case of | Before | contracts#23 |
|---|---|---|
| `approve_execution` | 10,768,309 | 2,593,252 |
| `create_proposal` | 5,060,490 | 1,266,076 |
| `accept_milestone` | 3,601,806 | 1,904,900 |
| `evaluate_project` | 2,738,434 | 1,576,848 |
| `create_project` | 2,278,545 | 776,494 |

15. **Nothing is refused, and two approvals are still over an ordinary
    block.** Four milestones with 14 requirements need 2,593,252 bytes, 106% of
    the limit, so a whole core. Three milestones with 12 need 2,192,710, which
    fits only without the gas margin. Teams, IDs, reservations and balances
    are the same as with the earlier contracts.
16. **The proof is charged by the access, not by the data.** Kreivo's weights
    charge 10,263 bytes plus the value's length for every storage read, write
    or existence check: the cost pallet-contracts benchmarks for a storage tree
    20 levels deep, applied to every access. Each call to a contract is charged
    its whole code, 162 kB for Project and 128 kB for Dao. The gas limit a
    transaction declares must cover that charge. The data is small: the largest
    extrinsic of the run is 872 bytes, the 193 together store a net 29 kB in
    1,095 storage entries (an entry is one key and its value), and the
    four-milestone approval stores 1,500 bytes in 68. Each existing task adds
    10,271 bytes to the next `create_task`: one access and its 8-byte record.
17. **Growth with the platform remains** (finding 11), except that
    `create_project`, `create_deposit` and `create_proposal` no longer grow
    with the directory:
    - An approval reads every worker who holds a required skill, about 31 kB
      each. With 92 workers the smallest approval needs 3.5 MB and a
      five-requirement one 8.2 MB.
    - `register_worker` shifts a sorted index, about 18 kB per registered
      worker: 2.05 MB with 92.
    - Every Project message loads the whole project, 10.3 kB per task.
    - The sorted indexes of projects, proposals, deposits and disputes shift
      the same way. That is read from the code; 14 projects do not show it.
    - The storage deposits the run's transactions take rose 8.6% (6.84 to
      7.43 KSM), since the ID registry stores one item per ID and Kreivo's
      deposit is per item.

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
of the marketplace run with the earlier contracts,
`market-contracts-2a74d50.out` the same run with contracts#23, `mlimits.out`
the limits and `wallet-test.log` the wallet signatures. The spike sources sit in `spike/` beside this note,
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

What makes the calls heavy, read in the source of the earlier contracts
(`abako-dev/contracts` at `a0532843`) and measured in findings 10, 11 and 16.
The proof charged follows the number of storage items a message touches, and
the code of every contract it calls.

1. **Check ID uniqueness by key, not by walking everything.** Done in
   contracts#23: one registry entry per public ID replaces
   `Dao::validate_public_ids`, which loaded every worker, every project's
   layout and submissions, every deposit and every dispute.
2. **Select without loading the directory.** Partly done in contracts#23: a
   message reads each worker, calendar and skill storage entry once, and
   coordinators have their own index. An approval still reads every holder of a
   required skill (finding 17). What is left is to keep the eligible accounts
   and their capacity per week in fewer storage entries (item 9), or to let the
   caller name the candidates and have the contract verify them, which changes
   who picks the team and is the owner's to decide.
3. **Split the approval.** Deferred. `approve_execution` assigns every
   milestone's team, reserves every calendar and takes the whole escrow in one
   transaction. The chain no longer refuses the largest one, but two are still
   over an ordinary block (finding 15). Decide once item 9 is measured: approve
   the proposal and escrow once, then assign and reserve one milestone per
   transaction.
4. **Load only what a message changes.** Partly done in contracts#23: a Project
   writes only the storage entries that changed. It still reads all of them on
   every message, so every task adds 10.3 kB to each later call.
5. **Page the lists.** `catalog`, `workers`, `projects` and `Project.view`
   return everything. Give them a start and a count.
6. **Seed the catalog after deployment**, in transactions, not in the
   constructor.
7. **Emit events** for each effect, with its entity, recipients and allocated
   IDs, and for each domain error that today exists only as return data.
8. **Stop trusting the caller for what the shell supplied**: derive selection
   seeds and IDs in the contract, and do not rely on the caller's identity to
   keep a read private.
9. **Pack the storage.** Proposed below, not approved.

The acceptance line for these is the first bullet under [acceptance](#acceptance),
measured again with `spike/src/bin/market.rs`.

### Packed storage (proposed)

The contracts keep one storage entry per small record: the average entry of the
run holds 27 bytes and costs 10,263 to touch. This proposal keeps every rule
and public message and changes where the data lives. Nothing in it is measured.
The estimates count accesses saved at 10.3 kB each, and the replay is the test.

**Project.** A project keeps a separate storage entry for each proposal,
milestone, requirement, required skill, assignment, rating, task, task
assignee, submission and evaluation, and loads all of them on every message:
about 60 reads for the four-milestone project before any work starts.

- One storage entry per proposal for its terms: milestones, requirements and
  skills.
  The four-milestone, 14-requirement proposal is about 1 kB.
- Per milestone, one storage entry for execution (status, assignments,
  ratings) and task pages, each a storage entry holding several tasks with
  their assignees. A task is 8 bytes and 32 more per assignee, and a full
  page starts a new one.
- One storage entry per submission with its ratings, and one per evaluation
  with its ratings.
- Every write checks the encoded size against a stated bound well under the 16
  KiB limit on a storage entry and fails with a domain error above it. For the
  terms and execution entries that error is a new limit on the size of a
  proposal and of a team, far above the scenario's, and like the calendar's cap
  it is a new rule.

The project then loads about 10 storage entries where it loads 60, and tasks
stop adding an access each. The loading code keeps its shape: one tree in
memory, saved by comparison. Loading only the storage entries a message needs
would save more and is a larger change; it can follow if the measurement asks
for it.

**Calendar.** A reservation writes three storage entries: the reservation,
the week's total and the week's key. Selection reads up to two per candidate
and week.

- One storage entry per worker and week: override capacity, total reserved
  and that week's reservations. The worker's header lists the weeks that have
  one.
- That entry needs a cap on reservations, since they are never released:
  128 per worker and week is 6 kB. That cap is a new rule. Without it, the
  reservations stay one storage entry each and only the total and the key merge.

For the four-milestone approval the two changes save about 100 accesses of
the 225 it makes, which would put it near 1.6 MB. If the measurement confirms
that, item 3 is not needed at this size.

**Dao indexes.** Workers, projects, proposals, deposits, withdrawals and
disputes each keep a sorted list with one storage entry per element, and an
insert shifts the elements after it.

- Keep the order, since `workers()` and `projects()` return it and selection
  depends on it, and store each list as sorted pages of up to 256 elements per
  storage entry. An insert rewrites one page, or two when a page splits.
- Keep a worker's roles and skills in the worker's record, and each skill's
  holders in pages. A candidate then costs about two accesses where it costs
  three. This lowers the growth of an approval with the directory; it does not
  remove it. Only naming the candidates (item 2) does.

**Dao's copies of Project data.** Dao stores each project's proposal,
milestone and submission IDs to work out which IDs a change adds or drops, and
each proposal's roles and skills to refuse deleting a catalog entry in use.

- Project sends the IDs it adds and drops, which it knows from the tree it
  loaded. contracts#23 already does this for submissions, which only append. The registry entry records the owning project, so a Project frees
  only its own IDs. The copies go.
- A proposal's roles and skills move to one storage entry per proposal.

**What reads the storage.** The provider decodes Dao's and Project's storage
entries directly (`facts.rs`, `project_storage.rs`, about 700 lines) and
checks that the indexes are sorted. Two ways to follow a new layout:

- *Read through the contracts' messages* (recommended). `workers`, `catalog`,
  `skill_requests`, `projects`, `disputes`, `calendar`, `financial_projection`,
  `Project.view` and `Project.reasons` already return most of what the
  provider reads. Missing: the next task ID, the origin of each skill and Dao's
  reason records. This is what the API must do on the chain anyway
  ([reads](#reads-and-notifications)), so the work carries over. The provider
  reads every fact on each request, so the time a read takes has to be
  measured first.
- *Rewrite the two codecs* for the new layout. No contract additions, and the
  code goes away with the provider in stage 3.

**Compatibility.** Public messages and the order of the lists they return do
not change. Stored data does, so it needs fresh data, as contracts#23 already
does. The hooks between Project and Dao change, and with them the ABI metadata
and the pinned artifacts.

**Order of work.** Project and calendar first, measured with the replay, then
the indexes and the copies. Each step keeps the comparison the first changes
used: the same teams, IDs, reservations and balances as the earlier contracts.

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
  [`results/market-final.out`](results/market-final.out) for the earlier
  contracts, and in finding 15 for contracts#23.
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
8. **The packed storage** proposed under [contract changes](#packed-storage-proposed):
   whether to do it, the new limits it brings (reservations per worker and
   week, the size of a proposal and of a team), and how the provider follows
   the new layout.
9. **Whether stage 2 may start before the last two approvals fit an ordinary
   block.** With contracts#23 nothing is refused, but decision 5 asks for
   ordinary blocks, and a whole-core transaction holds up the two blocks after
   it (see [risks](#risks)). The limits of finding 17 also apply: about a
   hundred workers.

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
