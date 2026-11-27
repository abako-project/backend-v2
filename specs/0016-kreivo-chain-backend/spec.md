# SPEC-0016: Kreivo as the only backend, through sube, with pallet-pass identity

Status: APPROVED — the owner approved stages 1 to 4 on 2026-11-27. Stage 5,
live Kreivo, is approved separately. Requested by the owner on 2026-11-20; the
[decisions](#decisions) carry their dates. The public API stays as it is,
password login included, and of a user's keys the backend keeps a session key.
Stages 2 to 4 change how users are authenticated, bring signing into the API
and remove services.
The contracts fit ordinary blocks since
[contracts#24](https://github.com/abako-dev/contracts/pull/24) at `27e109e`
(findings 18 to 24), which the backend pins
([#44](https://github.com/abako-project/backend-v2/pull/44)). Nothing here
authorizes publishing or deploying.

When approved this supersedes:

- SPEC-0001 (custodial signing), and in it REQ-014, "Passwords and other login
  credentials shall not derive, encrypt, replace, or determine the custodial
  signing seed": a password here determines a device key. Its second sentence
  stays true: changing credentials preserves the account.
- The passkey section of SPEC-0004 (AUTH-001) where it has the API verify the
  credential and excludes chain signatures and `Pass.register`.
- The execution, authority and persistence parts of SPEC-0013.
- The words "custodial wallet" and "custody account" in the descriptions of
  `contracts/openapi.json`.

SPEC-0015's ownership split (API content, contract facts) and the marketplace
rules of SPEC-0003/0005–0008 are unchanged.

## Outcome

One `api` process and PostgreSQL. The API talks to Kreivo through
[sube](https://github.com/virto-network/virto-sdk/tree/main/crates/sube); the
connection string selects the chain:

| `KREIVO_URL` | Chain | Use |
|---|---|---|
| `fork:<snapshot>` | In-process fork of Kreivo (sube-fork), instant blocks | Local development, tests, the mock that frontends integrate against |
| `wss://…` | A Kreivo node | Live |

The public REST/JSON and SSE API does not change: its routes, fields, statuses,
error codes and cookie session stay, so a frontend built for the present
backend works against this one ([compatibility](#compatibility)). What changes
is behind it.

The fork runs the real Kreivo runtime, so the same extrinsics, pallet-pass
checks, contract limits and fees apply in both. `services/wallet` and
`services/contract-provider` are removed, with their SQLite stores, the private
HTTP interfaces, the service token, the isolated polkadot-sdk workspace and its
advisory exceptions.

Users are pallet-pass accounts, and the chain verifies every credential. A
password is a key device derived from it; a passkey is a WebAuthn device. The
API holds one session key per login, limited to `Contracts.call`, and keeps no
other key of a user beyond the request or ceremony that needs it. Custodial seeds and the API's own verification of passwords and
passkeys go away.

The first delivery is the mock: the API on a fork, for frontends to integrate.
Live Kreivo is stage 5.

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

#### With packed storage

The same run on 2026-11-26 with
[contracts#24](https://github.com/abako-dev/contracts/pull/24) as merged at
`27e109e`, which packs Project and the calendar
([packed storage](#packed-storage)) and keeps every public message:

| Of 193 transactions | contracts#23 | Project packed | contracts#24 |
|---|---|---|---|
| Fit an ordinary block with the 10% gas margin | 191 | 193 | 193 |
| Need a whole core | 1 | 0 | 0 |

| Heaviest case of | contracts#23 | contracts#24 | Share of the limit |
|---|---|---|---|
| `approve_execution` | 2,593,252 | 1,359,571 | 56% |
| `accept_milestone` | 1,904,900 | 1,121,545 | 46% |
| `create_proposal` | 1,266,076 | 923,194 | 38% |
| `create_project` | 776,494 | 761,918 | 31% |
| `register_worker` | 678,375 | 680,186 | 28% |
| `evaluate_project` | 1,576,848 | 618,336 | 25% |

18. **Every transaction fits an ordinary block with the margin.** Packing
    Project alone already does it: its heaviest approval is 1,827,665 bytes,
    75% of the limit. Teams, IDs, reservations, balances and statuses are the
    same as with contracts#23. The run's storage deposits fall from 7.43 to
    4.44 KSM. Project's code is 156,166 bytes and Dao's 129,640.
19. **A contract's memory bounds what a message can load.** A contract has
    1 MiB, in Kreivo's runtime and in the provider, and ink's allocator never
    frees. A message loads every reservation of each week it reads. With a cap
    of 128 reservations per worker and week, one reservation over a 50-week
    window trapped once each week held 79. With the cap at 32 and two changes
    that cut the memory a reservation takes by a third, windows of 50, 150 and 170
    weeks fill to the cap and the next reservation is refused with
    `insufficient_capacity`; a 200-week window traps at 31 per week, in a
    message that needs 4.8 MB of proof. The cause was not observed directly:
    the traps moved by about the factor the two changes predict. The log is
    [`results/calendar-limits.out`](results/calendar-limits.out).
20. **`calendar` returns a bounded history.** With contracts#24 it returned
    5,440 reservations of one worker and trapped at 6,200; with contracts#23
    it returns 6,250, near what its return buffer holds. Reservations are
    never released, and once the provider reads calendars through that message
    ([#44](https://github.com/abako-project/backend-v2/pull/44)) a worker past
    that history fails every read. A paged read removes the ceiling.
21. **A Project's size is bounded by the contract's memory, not by a check.**
    Every Project message loads the whole Project. With one milestone, growing
    the Project in steps of 250, 100 and 25 tasks:

    | Assignees per task | Approval and acceptance work at | Trap at | Last task created |
    |---|---|---|---|
    | 1 | 4,000 | 4,090 | 4,090 |
    | 5 (acceptance only) | 1,200 | 1,300 | 1,658 |
    | 20 | 325 | 350 | 492 |

    So a coordinator can add tasks past the size at which the client can
    still approve or accept. `set_task_assignees` worked at every size, and
    removing assignees is the way back. With one assignee per task an
    acceptance fits an ordinary block at 2,250 tasks (2.33 MB of proof) and
    not at 2,500. The contracts of contracts#23 trap at the same sizes.
22. **`Project.view` returns up to about 290 kB.** It returned 290,645 bytes
    for 450 tasks of 20 assignees and trapped at 475, with contracts#24 and
    contracts#23 alike; the contract's return buffer holds 294,912 bytes. The
    provider reads every Project through it
    ([#44](https://github.com/abako-project/backend-v2/pull/44)), so one
    Project past that size fails every read.
23. **An approval whose candidates' weeks are full is refused, not trapped.**
    `assign_team`, the Dao part of an approval, was called for one requirement
    with every week of every candidate at the cap. It was refused with
    `no_available_candidate` up to the largest size tried: 12 candidates over
    24 weeks, 288 week entries holding 9,216 reservations. With one minute
    free in every week it succeeded at the same sizes. Proof bounds it first:
    every case of up to 144 week entries fit an ordinary block and none above
    180 did. When the first week is enough it takes 0.39 to 0.73 MB.
24. **Three size checks were exercised on the built contracts.** At `27e109e`: the
    sixteenth task of a page whose tasks have 24 assignees is refused with
    `storage_entry_too_large`, and a task of one assignee is then accepted. A
    calendar takes 104 overrides and not 105. A proposal of ten milestones
    takes 12 requirements each and not 13.

Findings 19 to 23 were measured on contracts#24 before its last two changes,
the cap on overrides and the room a proposal keeps for its team (findings 21
to 23 at `e218b0c`). They were not repeated at `27e109e`.

Not measured, on the built contracts:

- A calendar that cannot list another week, and the 12 KiB checks on a
  delivery, an evaluation and a calendar's header. Unit tests cover the first,
  and unit tests did not show finding 19.
- Weeks near the cap in the scenario, where each read of such a week costs
  about 1.5 kB more.
- A Project of several milestones or proposals at the sizes of finding 21.

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
`market-contracts-2a74d50.out` the same run with contracts#23,
`market-contracts-27e109e.out` with contracts#24 as merged
(`market-contracts-e218b0c.out` is the run before its last two changes),
`calendar-limits.out` finding 19, `project-limits.out` findings 21 and 22,
`approval-at-cap.out` finding 23, `bounds-27e109e.out` finding 24,
`mlimits.out` the limits and `wallet-test.log` the wallet signatures. The spike sources sit in `spike/` beside this note,
untracked, with the snapshot: they link GPL crates and are evidence, not backend
code. `main.rs` is the first run, `bin/market.rs` the marketplace,
`bin/mlimits.rs` the limits and `bin/wallet.rs` the wallet signatures.

## Requirements

### Compatibility

- `contracts/openapi.json` changes in descriptions, and the passkey routes
  gain the 409, 429 and 503 answers that other routes already document, since
  they now wait for a transaction. The contract tests of `crates/protocol`
  stay as they are, and a new test checks that the router and the document
  list the same routes.
- `scripts/poc-e2e.py` and `scripts/seed-dev-users.py` keep their requests to
  the public API and what they assert of its answers. What they do to the
  services changes: how the stack starts, the wallet's metrics they read, and
  the proxy that drops one of the provider's replies, which becomes a restart
  of the API between storing an extrinsic and learning its outcome.
- Two values change with the fresh data the switch needs: `accountId` is the
  pass account, and `providerInstanceId` names the chain and the Dao instance.
- On a fork a write takes as long as today. On a live chain it takes a block
  and its finality; no public field promises a time.
- One thing a user notices: a session lasts 15 minutes, the longest Kreivo
  allows today, where it lasts a day now. After it the API answers as it does
  for an expired session, and the user logs in again.

### Chain access

- A `kreivo` crate owns the sube connection on one thread and exposes `Send`
  handles with bounded queues. It prepares, submits and tracks extrinsics,
  dry-runs and reads contracts with `ContractsApi_call`, and follows finalized
  blocks. No other code holds a sube handle.
- A submission returns once the transaction is sent, and its inclusion and
  finality are reported as they happen, so reads and dry-runs do not wait
  behind a transaction. The crate today runs one request at a time and waits
  for finality.
- Fork support is a cargo feature. The mock build used by Compose, tests and
  the preview enables it; the live image does not, and links neither smoldot
  nor the dev operations.
- Startup checks the genesis hash, that the configured Dao exists with the pinned
  code hash, and that the runtime has pallet-pass, pallet-contracts and the
  asset extension. A mismatch fails startup.
- In fork mode a first start deploys and seeds the contracts, registers the
  administrator and mints the mock payment asset. Fork state persists with
  PostgreSQL and is saved with every block the API relies on; a store that
  does not match the fork fails startup instead of being reset.
- The fork's clock follows wall time, since sessions, calendars and receipts
  use the chain's time.
- ABI routing (`ProviderCommand` to contract and input) moves out of the
  provider into shared code without reproducing business rules.

### Identity and sessions

Every auth route keeps its path, request and response.

- **A principal is a pass account.** Its user ID is the BLAKE2b-256 hash of
  the normalized username, and its address is derived from that without a
  transaction, so registration returns `accountId` as it does today. Display
  data stays in PostgreSQL. PostgreSQL, not the chain, records who registered
  here; a username that already has a pass account on Kreivo answers
  `username_taken`.
- **A password is a key device.** The API derives a 32-byte sr25519 seed with
  Argon2id from the password, a random salt per user and a secret the process
  is given at start, passed as the function's own secret input. PostgreSQL
  keeps the salt, the parameters and the device's public key. It keeps no
  password hash and no seed.
- **The device key is derived when a request carries the password and dropped
  when the request ends**: registration, login, a password change, removing a
  passkey. Adding a passkey is the exception. It takes two requests and the
  second carries no password, so the key is kept in memory with the ceremony
  for its five minutes at most.
- **Registration** derives the key, has the pass account registered with it as
  the first device, and opens a session. **Login** derives the key, refuses a
  wrong password by comparing public keys (`invalid_credentials`, without a
  transaction), and opens a session.
- **A password change** is one batch that adds the device of the new password,
  with a new salt, and then removes the old one. The new salt is stored before
  the batch is sent and the old one kept until it is final. `accountId` does
  not change.
- **A passkey is a WebAuthn device**, added by the password device with a
  filter that allows contract calls and opening sessions, never as an admin
  device. The routes are today's: an addition to a password account,
  registered and removed with `currentPassword`, and a login by username. The
  API checks the origin, relying party and user verification of the browser's
  answer before submitting, since the chain does not check them on a login.
  PostgreSQL keeps each credential's ID and creation time.
- **A session key per login**, limited to `Contracts.call` and never an admin
  key. The session cookie and CSRF token are unchanged.
- **The cookie session ends with the chain's session**: 15 minutes on Kreivo
  today, a day once Kreivo allows it ([Kreivo changes](#kreivo-changes)). The
  chain removes a session only when it expires, so a logout drops the key and
  the cookie and leaves the session to run out. An account has ten sessions
  at a time; an eleventh login within 15 minutes answers `capacity_exceeded`.
- **Session keys are kept in memory** (decision 18). A restart of the API ends
  every session, and the API runs as one instance.
- **The administrator** is the pass account of the bootstrap username, with
  the password of the bootstrap file. It is the Dao's operator and acts through
  its session key like any user. There is one, as today. At the first start
  the account is registered, then given its balance, then made the deployer
  of the Dao: an address that already holds a balance cannot be registered.
- **Lost credentials are not recovered by the API.** pallet-pass has no
  recovery call; Kreivo governance decides case by case (decision 15).

### Fees, deposits and onboarding

A transaction's fee and the storage deposit of a contract call fall on the pass
account. What Kreivo offers, read in the runtime of spec 136 and not yet run:

- **Fees.** Governance publishes a usage-subscription offer and a community
  subscribes. A member of that community then pays no fee while the
  community's pool has room for the transaction's declared weight. The gas
  limit is therefore sized from the dry-run. The per-membership gas tank of
  earlier runtimes was removed at spec 135.
- **Storage deposits are not covered.** A contract call locks 6,666,666,660
  planck (0.0067 KSM) per storage entry it adds, from the caller's native
  balance: 200,145,999,654 planck (0.20 KSM) for `register_worker` in the
  measured scenario, and 4.44 KSM over its 193 transactions.
- **Registering a pass account** is open to any signed account, which pays the
  fee and a hold of 43,999,989 planck (0.000044 KSM) for its first account and
  10,666,656 planck for each further one.

The design, in two steps:

1. **On the mock**, the onboarding account registers each pass account, and
   the API then gives the new account its native balance with a fork
   operation. The onboarding account is a key the API makes at the first
   start and funds the same way. No community is involved. This is the first
   delivery, and it does not prove the fee path, the membership or the funding
   of deposits.
2. **With memberships** (stage 4, before live): the organization is a Kreivo
   community with a usage subscription, and its users are members without
   voting power. An onboarding account held by the API registers each pass
   account as a plain signer, adds it to the community through a proxy limited
   to `Communities.add_member`, and tops up its balance for deposits from its
   own, up to a cap per account. The order is always register, add, fund: an
   address that already holds a balance cannot be registered.

### Keys and dependencies

What the backend holds:

| Key | Held | For |
|---|---|---|
| A session key per login | In memory, for the session's lifetime | Contract calls as the user |
| A password's device key | In memory, while a request carries the password and for the five minutes of a passkey registration | Opening sessions, changing devices |
| The derivation secret | Given to the process at start | Deriving device keys from passwords |
| The onboarding account's key | A file | Registering accounts; from stage 4, adding members and topping up deposits |

New in the API, approved with this note: sr25519 signing (`subxt-signer`, in
the wallet today), the keyed use of Argon2id (`argon2`, already in the API),
and a reader of WebAuthn registrations, chosen in stage 1 and reviewed as a
dependency then. The `kreivo` crate brings sube and the pass crates under
decision 2.

### Operations

- Business writes keep the public operation reference, polling, statuses and
  receipt. Each field has a source on the chain:

  | Public promise | Source |
  |---|---|
  | `AwaitingSignature`, `ReadyToSubmit`, `Submitted`, `Finalized` | Preparing (dry-run, session key present); the signed extrinsic is stored; sent; final and visible to reads |
  | `receipt.origin`, `nonce` | The pass account and its nonce |
  | `receipt.providerInstanceId` | Set when the Dao is deployed, with a random part, so a fork made again is a new instance |
  | `receipt.finalizedAt` | The timestamp of the including block, which is the contract's time |
  | `receipt.createdEntityId` | The dry-run's effect: IDs are passed by the caller or derived from the seed it passes |
  | `receipt.outcome` `Failed(code)` | The dry-run's error code. An operation the dry-run refuses ends `Finalized` with that code and is not submitted: its receipt carries the account's nonce, which is not used up, and the time of the block the dry-run ran on |
  | Event cursors, notifications, recipients | The API keeps the event log in PostgreSQL and assigns cursors at finality, from the effect stored with the extrinsic |
  | Idempotency, one operation in flight per account, queue caps | Unchanged |

- The exact signed extrinsic and the dry-run's effect are stored before
  submission. After an interruption the same bytes are resubmitted or their
  inclusion is found; an operation is `OutcomeUnknown` until that resolves,
  and its mortality bounds how long that can last.
- A call that reverts on the chain after a clean dry-run, runs out of gas or
  exceeds its deposit limit ends `Finalized` and `Failed` with a general code,
  since a reverted call leaves no return value. Every call carries an explicit
  deposit limit.
- On a fork the dry-run and the submission are one step, so the effect,
  including an allocated task ID, is exact. A live chain needs contract events
  for that (stage 5).
- Transactions a device signs (login, password change, passkeys) go through
  the same queue of the account, since they use its nonce.
- The seed of a selection is generated by the API, as the provider does today.
- Exact money rules are unchanged: checked integers, decimal strings.

### Reads and notifications

- Reads keep the shape of today's snapshot. It is built from contract messages
  at a finalized block and kept in memory, and refreshed when a finalized
  block calls the Dao or a Project. Today the whole state is fetched on most
  requests and once a second per event stream. How much a refresh costs as the
  organization grows is not measured.
- An operation is reported `Finalized` only once the reads include its block.
- Notifications derive from the API's event log. Until the contracts emit
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
9. **Pack the storage.** Described below. Project and the calendar are
   approved (decision 8) and merged in contracts#24; the Dao indexes and
   Dao's copies are proposed.

10. **Name the operator.** The Dao's operator is the account that deployed
    it. On Kreivo a contract is instantiated by its community's account
    (finding 2), so the operator becomes a constructor argument that the
    operator can hand over. The mock does not need it: there the
    administrator's pass account deploys the Dao with a fork operation.

The acceptance line for items 1 to 9 is the first bullet under
[acceptance](#acceptance), measured again with `spike/src/bin/market.rs`.
Items 7 and 10 and the seeding of finding 3 are needed for live Kreivo, not
for the mock.

### Kreivo changes

Read in the runtime of spec 136 (frame-contrib `4d16b1e`); none is made yet.

1. **Longer sessions.** `MaxSessionDuration` is 150 relay chain blocks, 15
   minutes. It becomes at least the cookie session's 24 hours, 14,400 blocks.
2. **A session its owner can end.** An account has ten session slots and only
   Root removes a session before it expires, so day-long sessions run out. The
   account or the session key removes its own session, which is also what a
   logout needs. Failing that, more slots than the 15 cookie sessions the API
   allows a principal.
3. **A proxy type for onboarding** that allows `Communities.add_member` and
   nothing else. The existing types are `Any`, `NonTransfer`, `CancelProxy`,
   three for assets and `Collator`.
4. **Votes by rank.** The community decides by rank, and a regular user, a
   client for example, has rank 0 and no voting power (decision 19). The
   communities pallet may need adjustments for that: it was not read through.

What the owner sets up on live Kreivo, after the runtime upgrade:

- Through governance, a usage-subscription offer.
- The project's community: registered (0.5 KSM), subscribed to the offer, with
  membership stock issued (one existential deposit each, at most 256 a call)
  and its voting rules.
- The onboarding account: a proxy of the community of the type above, with a
  balance for registrations and top-ups.
- The Dao and Project code published and instantiated through the contracts
  store, after contract changes 7 and 10 and with the catalog seeded in
  separate transactions.
- The payment asset the Dao uses.

A fork taken after that carries all of it.

### Packed storage

The contracts keep one storage entry per small record: the average entry of the
run holds 27 bytes and costs 10,263 to touch. This proposal keeps every rule
and public message and changes where the data lives. The owner approved the
Project and calendar parts on 2026-11-25 (decision 8). They are merged in
contracts#24 and measured in findings 18 to 24; the text below describes them
as built. The Dao indexes and Dao's copies of Project data stay proposed and
unmeasured: their estimates count accesses saved at 10.3 kB each.

**Project.** A project keeps a separate storage entry for each proposal,
milestone, requirement, required skill, assignment, rating, task, task
assignee, submission and evaluation, and loads all of them on every message:
about 60 reads for the four-milestone project before any work starts.

- One storage entry per proposal: its terms and, for each milestone, its
  status, team and ratings. The scenario's largest, four milestones and 14
  requirements, is 1.8 kB.
- Per milestone, task pages: sixteen tasks with their assignees to a storage
  entry, and a full page starts a new one.
- One storage entry per submission with its ratings, and one per evaluation
  with its ratings.
- Every write checks the encoded size against 12 KiB, under the 16 KiB limit
  on a storage entry. A proposal is checked when it is created or updated,
  with the room its team, ratings and change request will take, so a stored
  proposal can always be approved and rated. Above the bound it fails with
  `proposal_too_large`: about 120 worker slots over ten milestones. Any other
  entry fails with `storage_entry_too_large`: about 23 assignees per task in
  a full page.
- Deleting a task moves the later ones up a place. It is refused with
  `storage_entry_too_large` when a page would then not fit its tasks, until
  their assignees are reduced.
- Nothing bounds the number of tasks (finding 21).

Tasks stop adding an access each. The loading code keeps its shape: one tree
in memory, saved by comparison. Loading only the storage entries a message
needs would save more and is a larger change.

**Calendar.** A reservation writes three storage entries: the reservation,
the week's total and the week's key. Selection reads up to two per candidate
and week.

- One storage entry per worker and week: the total reserved and that week's
  reservations. The worker's header holds the capacity, the overrides and the
  weeks that have reservations, in 12 KiB.
- A calendar takes up to 104 overrides and fails with `calendar_too_large`
  above. The header then has room for about 2,300 reserved weeks. A calendar
  with no room for another week counts as full in the weeks it has no
  reservation in.
- A week holds a capped number of reservations, since they are never released
  and a message loads all of a week's (finding 19). A week at the cap counts
  as full: `calendar_available` returns 0 for it with minutes left, and a
  reservation whose window has no other week fails with
  `insufficient_capacity`. A reservation takes the earliest weeks of its
  window, so a busy coordinator's collect in single weeks. A coordinator
  whose current week is at the cap is not selected, and `create_project`
  fails when all are. The cap is 32 (decision 8).

The four-milestone approval now needs 1.36 MB, so item 3 is not needed at
this size.

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

- *Read through the contracts' messages* (chosen in decision 8). `workers`, `catalog`,
  `skill_requests`, `projects`, `disputes`, `calendar`, `financial_projection`,
  `Project.view` and `Project.reasons` already return most of what the
  provider reads. Missing: the next task ID, the origin of each skill and Dao's
  reason records. This is what the API must do on the chain anyway
  ([reads](#reads-and-notifications)), so the work carries over.
  [#44](https://github.com/abako-project/backend-v2/pull/44) does it for
  calendars and Project trees, which is what contracts#24 moves, and pins the
  merged contracts. The provider reads every fact on each request: a read
  then takes about 1.1 ms more per worker and 2 ms per project in a release
  build. It answers the contracts' size limits and `insufficient_capacity` as
  failed operations.
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
   Left, experiments first:
   - A password device registers, opens a session, adds a passkey with a
     filter and changes the password.
   - A passkey end to end, with a software authenticator and with a browser.
   - The mock's first start in the order the administrator needs: register,
     fund, deploy the Dao as that account.
   - A privileged call on the fork through the runtime's dry-run API, which
     stage 4 needs.
   - Then the crate: submissions that do not wait, following finalized blocks,
     recovery of an interrupted submission, the wall clock, saving the fork
     with each block, WebAuthn devices and device filters, the mock's
     bootstrap, and the shared ABI routing.
2. **API on the fork.** Waits for the approval of this note.
   - A test that the router and `contracts/openapi.json` list the same routes.
   - One interface in the API in front of every wallet and provider call
     (about twenty call sites), with today's services behind it and the test
     fake implementing it. No behaviour changes.
   - Kreivo behind that interface, chosen by configuration: passwords, then
     passkeys, reads, writes, notifications.
   - Done when the API's tests and the end-to-end scenarios pass on the fork
     with their requests and assertions unchanged.
3. **Switch and removal.** Compose, the preview and CI run the fork. Delete
   `services/wallet` and `services/contract-provider`, their images, Compose
   services, secrets, policy files and documents. Fresh data.
4. **Memberships on the fork.** The community, its subscription and the
   onboarding account, set up with privileged fork operations or taken from
   live Kreivo once the owner has set it up there. Users then pay no fee and
   are topped up for deposits as on live.
5. **Live readiness**, separately approved: the Kreivo changes and setup,
   contract changes 7 and 10, split catalog seeding, recovery against a real
   node, whether login waits for inclusion or finality, and wallet signing.

Each stage ends with the independent review SPEC-0013 requires.

## Acceptance

- Every operation of the marketplace scenario fits a normal Kreivo extrinsic
  (0.467 s, 2,446,677 bytes of proof) on the fork, or is listed with its
  measured weight as a contracts blocker. The list is in finding 10 and
  [`results/market-final.out`](results/market-final.out) for the earlier
  contracts, and in finding 15 for contracts#23. With contracts#24 the list
  is empty (finding 18).
- `contracts/openapi.json` differs from today's only in descriptions, and the
  contract tests of `crates/protocol` pass unchanged.
- The end-to-end scenarios and the seed script pass in fork mode with their
  requests and assertions unchanged, from registration to settlement, with a
  password and with a passkey.
- A password change keeps `accountId`, and the old password no longer opens a
  session.
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
3. **The API holds a session key per login**, limited to `Contracts.call`.
   Business routes keep their shape and `accountId` is the pass account. The
   same day's choice to remove the password routes and make login a device
   ceremony in the browser was replaced on 2026-11-27 (decisions 10 to 12). A
   browser-held key remains the live target.
4. **Wallet login needs the authenticator to accept wrapped signatures.**
   With passwords kept (decision 12) it is no longer part of the first
   delivery and moves to stage 5. Kreivo's substrate-keys authenticator
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

Decided by the owner on 2026-11-25:

8. **Project and the calendar are packed**, as described under
   [contract changes](#packed-storage), and the provider follows by reading
   them through the contracts' messages. The Dao indexes and Dao's copies of
   Project data are not part of this. The values of the two limits it brings
   (reservations per worker and week, the size of a proposal and of a team)
   are confirmed by the owner before the contract change is proposed for
   merging. The owner confirmed the 12 KiB bounds, sixteen tasks per page and
   128 reservations per week the same day. On the built contracts a 50-week
   reservation traps before its weeks hold 128 (finding 19). On 2026-11-26
   the owner confirmed 32 reservations per week and 104 overrides per
   calendar, and merged contracts#24.
9. **Stage 2 may start before the last two approvals fit an ordinary block.**
   Stage 3 keeps decision 5's condition: the provider stays until the scenario
   fits ordinary blocks. Until then the whole-core approval is listed as a
   contracts blocker (see [acceptance](#acceptance)), since a fork accepts it
   in any block and Kreivo in one of three (see [risks](#risks)). The limits
   of finding 17 still apply: about a hundred workers. Stage 2 still needs the
   approval of this note.

Decided by the owner on 2026-11-26 and 2026-11-27:

10. **The public API should not break.** Ideally a frontend does not notice
    the switch. Changes are kept to the minimum, but nothing is kept that
    works against the chain's design, authentication above all.
11. **"We only want to handle temporary session keys in the backend."** What
    the backend holds under this design is listed under
    [keys and dependencies](#keys-and-dependencies).
12. **Passwords stay for now**, as a key derived from the password and
    registered as the account's device. Removing them needs a longer
    discussion with product.
13. **Onboarding has a backend account**, a proxy with limited permissions.
14. **"Runtime changes should be fine"**, asked about longer sessions, a
    session its owner can end and the onboarding proxy type
    ([Kreivo changes](#kreivo-changes)). They come later: for now the backend
    works with 15-minute sessions.
15. **Lost credentials are not handled now.** Kreivo governance can help.
16. **Users join the project's community without voting power.**
17. **The first deployment is the mock on a fork**, where accounts and setup
    are edited directly. The owner creates the project's community on live
    Kreivo and can fund the Dao's account at first; the fork can then be taken
    from that state.
18. **Session keys are kept in memory for now.**
19. **The community votes by rank.** Regular users have rank 0 and no voting
    power; the communities pallet may need adjustments.
20. **This note is approved**, and with it the new uses listed under
    [keys and dependencies](#keys-and-dependencies).

Open:

7. **What the fork adds to the dependency findings.** Through smoldot:
   `constant_time_eq` 0.1.5 under CC0-1.0, which the policy already rejects for
   five other crates pending the owner's decision, and `libsecp256k1`, reported
   unmaintained. Neither is silenced. Both come only with the `fork` feature.

Also open, from the design of 2026-11-27:

- **The contract changes for live** (7 and 10, and the catalog seeding) and
  the Kreivo changes: when they are made, and by whom.
- **Who funds users' deposits on live, and the cap per account.**

Defaults, unless the owner says otherwise:

- **Operator.** The administrator's pass account is the Dao's operator
  (requirements). The API holds no operator key. On live the operator is named
  at deployment (contract change 10).
- **Effects without events.** The dry-run's effect is stored with the
  extrinsic and applied at finality. It is exact on a fork, where the dry-run
  and the submission are one step; live needs contract events.
- **Snapshot for CI.** A pinned 25 MB artifact with its hash, refreshed after
  Kreivo runtime upgrades, rather than a download from the live node per run.

## Risks

- A session key held by the API can call any contract as the user until it
  expires, including with attached value; the filter restricts the call, not
  its destination. With the session changes
  that is up to a day.
- A transaction that needs a whole core waits for the first block of one and
  stops the collator building the other two. Several in a row slow the chain
  for everyone.
- Seeds chosen by callers and reads open to any origin (finding 14) need
  contract changes before a public chain; the API cannot enforce them.
- Nothing bounds a Project's tasks or releases a reservation. A coordinator
  can grow a Project until its client cannot approve or accept (finding 21).
  While the provider exists, one Project or one worker's history past the
  sizes of findings 20 and 22 fails every read, not only its own.
- Dao refuses an account's first payment when the account already holds the
  payment asset, so anyone can block a worker's first payment by sending them
  one unit. Read in the contracts' `Bank::provision`; not exercised.
- A snapshot contains live state and the live runtime at one block. A runtime
  upgrade can change pallet-pass or contract behavior before the snapshot is
  refreshed.
- Live latency is block time and finality, not milliseconds; login submits a
  transaction, and users need native balance before their first write.
- sube-fork is 0.x and its state roots are not authoritative.
- A password is an admin key of its account on a public chain. Whoever
  guesses it can use the account outside the API, and the API cannot revoke
  it. A copy of the database (salts and public keys) allows guessing offline
  only together with the derivation secret. Losing a salt or that secret
  locks the account, and pallet-pass has no recovery call.
- While a request that carries a password runs, the API holds that account's
  admin key, as the wallet service holds every key today.
- Usernames are global on Kreivo. Anyone can register a username first, or
  block it by sending its address the existential deposit, the
  administrator's included.
- Until stage 4 the mock gives accounts their balance directly, so the fee
  path, onboarding and deposit funding are not exercised.
- Topping up deposits hands out native balance: a worker's registration locks
  0.20 KSM, and a user's device can move an unlocked balance out.
- The design was read in the runtime's code. A password device with a
  day-long session, a passkey with a filter, a member paying from its
  community's pool through a session key, and privileged calls on the fork
  have not been run.
