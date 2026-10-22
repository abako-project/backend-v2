# ink! v5 compatibility proof

This candidate follows [SPEC-0013](../../specs/0013-ink-v5-sandbox/spec.md).
It runs production WASM in a native FRAME runtime without starting a node. It
is deliberately a separate Cargo workspace and lockfile: the application keeps
its current dependencies, authoritative Rust engine and startup behavior.

```sh
bash scripts/verify.sh ink-sandbox
```

The original M0 proof executes Calendar → Ratings and Project → Calendar calls, direct
caller asset deposits, contract balance queries and payouts using real
`pallet-assets`. It checks a failed deposit after tentative task mutations,
revert flags, finite gas, denied origins, malformed selectors and restoration
of contract Mapping data from a serialized raw-trie checkpoint. Contracts use
the production `cfg(not(test))` code paths, not ink's offchain test bypasses.
A separate v5 failure probe checks explicit traps, nested domain rejections and
an endless storage-writing loop, including rollback after gas exhaustion.
The marketplace fixture exercises **legacy** contract behavior to establish VM compatibility;
it does not establish parity with the current application or authorize switching.

## M3 Calendar slice

The separate `calendar-v5` candidate implements integer minutes, ISO weeks,
weekly overrides and append-only reservations from the current app. Its real
WASM and the Rust engine independently replay
[`calendar/fixtures/parity.json`](../../services/mock-provider/src/calendar/fixtures/parity.json).
The cases compare allocations, availability, capacity changes, domain errors
and retained project/milestone/requirement references. Additional WASM tests
exercise caller restrictions, query isolation, malformed weeks, metering,
competing SQLite workers and restart. The old four compatibility artifacts stay
unchanged. Directory and signed candidate routing are covered below. The older signed
Calendar fixture continues testing the explicitly limited legacy contract.

Directory will create one Calendar instance per worker. Only the worker or its
creating Directory can change capacity; only an immutable reservation controller
can reserve. These tests pass explicit fixture accounts as those capabilities.
They do not implement Project permissions or expose a new provider/browser API.
No calendar release operation is added: the approved rules retain consumed hours.

Adding the bundle changes the pinned candidate cohort identity. Existing private
candidate databases fail closed; use a new disposable fixture database. Ordinary
mock-provider SQLite data and its Rust authority remain unchanged. Large-state
import and performance budgets remain open. See the
[slice handoff](../../progress/handoffs/SPEC-0013-M3-CALENDAR.md).

## M3 Directory slice

`directory-v5` owns catalogs, descriptive skill/role associations, worker
registration/qualifications, mode/eligibility, and skill requests/moderation.
Registration creates an actual `calendar-v5` child in the same transaction.
Worker views read that Calendar rather than duplicating capacity/reservations.
Fixed constructor capabilities control reservation and retained proposal references;
Real Project integration is covered by M5/M6 below; no browser endpoint is added.

The shared [Directory trace](../../services/mock-provider/src/domain/fixtures/directory-parity.json)
has 48 transitions replayed independently by Rust and real WASM. It compares error
codes, effect IDs/kinds/recipients and SHA256 of the full SCALE catalog/worker/request
projection after every step. IDs are captured fixture inputs; this does not claim
signed admission, receipt/event-cursor or RNG bridge coverage. Additional WASM tests
exercise request visibility, proposal-reference guards, immutable caller capabilities,
nested overcommit errors and rollback of a tentative Calendar on parent failure.
Raw-trie restart is checked during the trace.

The v5 compiler uses Unicode 16 while the app uses Unicode 17. The candidate
includes a reproducible, dependency-free compatibility layer for changed
lowercase mappings and final-sigma properties, with the Unicode license notice.
The canonical source documents generation and exhaustive cross-toolchain checks.
The raw Directory slice did not change dependencies; the signed bridge below
reuses the application RNG in the SDK lock without adding policy exceptions.

See the [Directory slice handoff](../../progress/handoffs/SPEC-0013-M3-DIRECTORY.md)
for canonical source, Unicode and artifact reproduction evidence.

## M3 signed Directory bridge

`Worker::start_directory_fixture` deploys the pinned Directory and Calendar code
from explicit catalog/controller inputs. It uses the same `provider-execution`
shell as Rust for all nine Directory/Calendar commands. The verified user calls
Directory directly; contracts check domain permissions. The bridge decodes effects
and errors and supplies the first ID from the version-1 locked native RNG stream.
Ordinary execution captures inputs internally. `execute_directory_with_inputs`
exists only for trusted comparisons in this standalone workspace, with no HTTP route.

The [signed trace](../../services/mock-provider/src/domain/fixtures/directory-signed.json)
contains 49 real signed calls, using disposable fixture keys. Both engines read
it independently. Every step compares the complete M3 projection and actual shell
history, including receipts, retained inputs, nonces, timestamps and event cursors.
Additional tests cover signature/envelope rejection, request visibility, replay,
query isolation, child rollback on SQLite failure, metering, restart and competing
writers. Ordinary domain rejection commits only a failed receipt and nonce;
unknown ABI/invariant/child-deployment failures abort without a shell write.

The runtime, actual shell and outcome commit together. Raw mutation requests cannot
write signed stores. Startup checks the fixture configuration, root, generation
history, runtime identity and pinned Directory code. The added RNG packages match
exact versions and checksums already in the app; see the [review](dependencies.md)
and renewed exact-lock [policy record](policy/README.md). No exception was broadened.

The working application still selects Rust. Other domains return `IncompleteCoverage`;
no browser contract, provider selector or hybrid fallback is added. Scores/matching,
Project/reference-controller integration, import capacity, performance and whole-provider
promotion remain later gates. See the [M3 signed handoff](../../progress/handoffs/SPEC-0013-M3-SHELL.md).

## M4 reputation and matching

Directory now stores the separate exact worker/coordinator accumulators and the
same validated score policy. Its signed bridge adds `SetScorePolicy`; root authority,
receipt/nonces, replay and event kind match Rust. No reputation is fabricated on
registration or settlement. Internal Project-controller contributions use committed
minutes and current relative vote weights; replay does not add them again and
policy edits leave past accumulators unchanged. Actual completed-project evaluation
participants and authorization remain M6.

The [M4 trace](../../services/mock-provider/src/domain/fixtures/matching-parity.json)
contains 172 component actions replayed independently by native Rust and real WASM.
It compares outcomes, exact policies and complete M3 projections including scores
and reservations after each action. It exercises skill intersections, descriptive
roles, relevant-mode scores, earlier-team preference, distinct slots, capacity,
zero weights, qualifications/index edits and failure across multiple milestones.
Sixty-four tied coordinator seeds also compare the following project ID. Seventy-nine
workers exercise candidates beyond a short list. Dense skill entries avoid a
packed-vector storage ceiling or a fixed search/truncation limit.

Directory is the immutable reservation controller of its Calendar children. Only
its fixed Project-controller capability can request matching/reservation. The worker
can still change its own capacity. Direct external reserve calls are rejected;
M3 fixture tests now use the same Directory controller path. Matching reserves
all milestones in one nested transaction; competing SQLite workers observe committed
capacity, and restart retains it. M5/M6 must connect real Project calls to these
hooks before whole-provider parity can be claimed.

The locked version-1 tie sampler preserves native behavior, including rand's documented
minute default single-sample bias. This is parity evidence, not strict mathematical
unbiasedness; changing it requires coordinated/versioned input semantics.

The canonical build reuses the four exact RNG versions already reviewed in the app
and SDK. The SDK/app locks and exceptions are unchanged. Updated immutable artifacts
change candidate identity and require fresh private fixture databases. The fresh
source archive reproduces both Directory and Calendar WASM/ABI. See the
[M4 evidence](../../progress/handoffs/SPEC-0013-M4.md). Rust remains authoritative.

The committed `fixtures/manifest.json` pins canonical source commit, toolchain,
artifact hashes and per-contract dependency locks. Tests verify artifact and
lock hashes before instantiation. With cargo-contract 5.0.3, Rust 1.88.0 and its
WASM target installed and the manifest source commit available locally, reproduce the WASM and ABI from a local canonical checkout:

```sh
python3 tools/ink-sandbox/rebuild.py ../contracts --offline
```

Omit `--offline` to fetch missing locked build dependencies. This builds committed
source in a disposable directory and compares the resulting WASM hash and ABI;
it never edits the canonical checkout or refreshes committed artifacts. Native
proof tests use Rust 1.96.1. Contract compilation is a separate artifact-build
step, never part of provider startup.

The runtime follows pallet-contracts 43's coherent dependency family rather than
stock ink_sandbox 5.1.1's FRAME 32 family. Its minimal asset extension implements
Kreivo extension 0, functions 0/1/2 (`Here(u32)` only), including exact u128/SCALE
encoding, caller/address semantics, error status codes and `Preserve` transfers.
Other extensions/assets fail explicitly. It does not emulate Kreivo's community
upload permissions, XCM, block consensus or production transaction fees.

Native execution/storage balances are technical fixture funds. Asset 1 balances
are separate. In particular, `Preserve` requires a minimum asset balance; the
legacy Project deliberately leaves one asset unit after payout. The current
application permits spending the full visible balance. The M6 candidate implements the approved separate technical-reserve policy and
checks full visible spending/payouts against actual asset balances. Legacy fixtures
retain their original behavior; no user payout is reduced.

The candidate now has a dedicated worker and a separate SQLite store. Typed
call/instantiate requests have an eight-item queue, 64 KiB inputs/outputs,
1 MiB shell-envelope limits, bounded gas and bounded checkpoint reconstruction.
No unmetered callback or caller-supplied WASM is accepted. The worker verifies
compiled-in bundles and their build locks before opening candidate state.
Queries run on discarded state. An expected revision and `BEGIN IMMEDIATE`
serialize competing workers; the full runtime, opaque shell envelope and exact
execution outcome commit in one SQLite transaction. The worker closes admission
at shutdown, finishes any executing transaction and rejects queued work. Call
`shutdown().await` to observe its join; dropping the owner signals cancellation
and the worker observes/logs its result without blocking a Tokio core thread.
A lost response never proves non-execution.

Checkpoint version 2 includes sorted raw trie nodes (with child tries), root,
runtime/configuration/lock identity, bundle-manifest identity and a corruption
digest. Decoding bounds bytes, allocation, nodes and reference counts; restore
checks node hashes and traverses every top-level and child key. Corrupt,
unsupported or incompatible state rejects startup without resetting genesis.
The old version-1 proof format was never a provider format and is not migrated.
The existing Rust provider's JSON/SQLite format is unchanged.

Milestone 2 also uses the real `provider-execution` shell shared with the
working Rust provider. `Worker::start_calendar_fixture` opens explicit genesis
fixtures, verifies signatures, admits/replays operations and commits the runtime
with actual nonces, receipts, captured inputs and events. Clients cannot supply
transport outcomes. Raw mutation admission rejects signed fixture stores; their
mode, root, checkpoint and pinned Calendar code are checked on startup.

The fixture bridge supports only `SetCalendar` whole-hour weekly capacity with
no overrides, against pre-registered fixture accounts and explicit calendar IDs.
Other commands or unsupported shapes return `IncompleteCoverage` with no nonce,
receipt or business write. This demonstrates shell/ABI integration; it does not
replace the M3 Directory bridge above. There is no public raw-contract
endpoint, sandbox authority selector, shadow success claim or frontend change.
Private library diagnostics expose revision, identity, root and size only.

The owner approved the exact [SDK exceptions](policy/README.md) for integration
work on 2026-10-22. Check the expiring, exact-lock SDK policy separately:

```sh
bash scripts/verify.sh ink-sandbox-dependencies
```

The root application graph adds only the shared shell crate; it acquires no SDK
packages. Its existing advisory/license findings are not waived. The full signer
remains in wallet/app tests; the shared verification shell uses the exact same
Schnorrkel primitive and context without importing key derivation into the SDK.

The candidate SQLite schema distinguishes raw, legacy Calendar and Directory signed fixture modes.
Earlier proof/candidate SQLite files without the mode column reject startup;
these were never application provider data. Runtime/lock identity changes also
reject old candidate checkpoints. Existing app SQLite/JSON opens unchanged.

`NoRandomness` is still unused v5 host scaffolding, not matching entropy.
Host-call weights are conservative finite proof limits, not production
benchmarks. Passing tests establishes neither application parity nor the
specified latency budgets. Full import/rollback and domain contracts remain later work.

See [dependency review](dependencies.md) and the recorded implementation evidence
in [the handoff](../../progress/handoffs/SPEC-0013.md) before promoting this runtime into
the provider.

M5's explicit `DirectoryFixture { projects: true, .. }` adds the pinned v5
Project factory and instances. Signed calls route to Directory for creation and
policy changes, then directly to Project as the verified user. Address lookup
and execution share one gas budget. Contracts own unpaid planning, proposals,
revision checks, optional tracking tasks, estimates, cancellation and reasons.
Directory owns the public registry, global IDs/task counter and catalog references.
`marketplace_projection` is a private fixture diagnostic; it does not add an HTTP
endpoint. Money/execution commands return `IncompleteCoverage` without a write.
Legacy planning escrow cannot be cleared until money coverage exists.

The frozen 74-call native/VM trace compares complete catalog, worker, request,
Project and reason projections plus the shared shell after every call. It covers
retained task storage, failed counter allocation, policy capture, exact rounding,
dates, optional tasks, permissions, revisions, reference cleanup and restart.
Factory SQL-failure injection verifies that child creation, registration and the
shell all roll back; competing workers replay a single successful commit. Direct
contract calls check factory/ID capabilities and discarded query writes.


## M6 execution and money fixtures

`DirectoryFixture { projects: true, money: Some(mock_funding), .. }` enables
explicit financial coverage. Directory owns visible integer KVN accounting and
simulated Bramp requests. Genesis prefunds an invisible physical inventory;
contracts separately provision each account/Project's technical minimum. The
same Kreivo extension moves actual assets. Verified clients call Project directly
for deposits, so the factory never pays on their behalf. Free balances, escrow,
holds and visible minted supply conserve exactly, without a payout haircut.
`Some(false)` disables the test-only funding command while keeping normal Bramp.

Projects implement captured execution funding, full-team reservations, sequential
milestones, delivery/rejection/resubmission, current and retained legacy settlement,
final evaluations, formal disputes and frozen cancellation. Cases cannot resolve
or unlock funds. The 129-call signed native/WASM trace, with 14 backward-clock abort probes, compares the complete
marketplace, balances/issuance/requests, cases and shared shell after every call
and across restart. Financial diagnostics read one SQL snapshot. Actual tests
check physical balances/minima and supply, SQL rollback during multi-recipient
settlement, competing full withdrawals, metered approval failure and private reads.
A nested exhausted ink call may surface as an outer contract trap; either known
metered failure commits only its failed receipt/nonce, with no business effects.

An immutable stateless `references-v5` contract shares the backend's exact URL
parser, including Unicode hosts and retained digest slots in surrounding DTOs.
Embedding it in Project exceeded Kreivo's pinned 192 KiB code cap. Splitting the
parser keeps Project (~145 KiB), Directory (~126 KiB) and validator (~155 KiB)
inside that unchanged cap, with no extra process or node. Directory pins its code
once; direct query tests compare native URL acceptance and reject replacement.
All eight artifacts remain immutable; the new parser uses the already reviewed
SDK registry cohort and changes no app/SDK lock or dependency-policy exception.

Configuring Project code permanently disables the early fixtures' fixed external
controllers. Real hooks accept registered Projects; finance cannot be enabled
before Project/reference code configuration. Older isolated fixtures retain their
explicit test capability. Unsupported component coverage still commits nothing.

This remains private fixture coverage. M7 complete import, sealed migration,
large-state/input acceptance and current-state rollback, performance budgets and
M8 whole-provider integration/promotion remain separate gates. Rust stays the
application authority. No public protocol, frontend flow or application storage
format changes in M6. See [M6 evidence](../../progress/handoffs/SPEC-0013-M6.md).

## Complete retained-state migration candidate (M7)

`Worker::import_archive` creates a fresh candidate store from the private typed
`ArchiveV1`. Its caller must first validate the archive with the native bridge,
pause writes, drain pending operations and reconcile uncertain outcomes. There is
no HTTP import endpoint. `Worker::reopen` requires an existing sealed store;
imports reject populated destinations and never regenerate incompatible state.
The default application remains Rust.

Directory alone creates dormant Calendar/Project instances. It imports balances,
escrow, holds, histories, exact score accumulators, categories and task counters
without replaying old operations, payments, events or evaluations. Historical
evaluation markers explicitly prevent a second contribution; no old-policy vote
hash is invented. Directory seals its import capability permanently and activates
its children only after the complete archive matches. SQLite then commits the
runtime and retained shell together.

`worker::archive` decodes the pinned storage-v5 cells using pallet-contracts'
read-only storage API. It does not execute business rules. Cells remain <=16 KiB;
8 KiB pages and streaming contract storage admit near-256 KiB signed profiles
without relaxing Kreivo's 16 memory pages or 192 KiB code limit. Complete reads
have a cumulative 64 MiB cell budget and a 64 MiB archive limit. The existing
runtime checkpoint and shell have separate 64 MiB caps. Admission/serialization
failure commits no candidate state; it never truncates data or claims agreement.
Successful whole-candidate writes must remain completely exportable before they
commit. The `comparison` diagnostic compares complete archives and exact outcome
classes; missing coverage or either missing export is `Incomplete`.

The signed migration trace imports six historical snapshots, compares all fields
after another 59 sandbox operations, preserves the near-limit legacy error
priority and exports the **current** state for rollback. Rehearse the cross-engine
rollback with ordinary commands (use fresh paths; the archive contains private
provider data):

```sh
sandbox_archive=$(mktemp)
SANDBOX_ROLLBACK_OUTPUT="$sandbox_archive" cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked --test migration complete_shadow_trace_preserves_current_sandbox_writes_for_rollback
SANDBOX_ROLLBACK_ARCHIVE="$sandbox_archive" cargo test -p mock-provider --all-features --locked --test migration rollback_current_wasm_state_accepts_new_native_work -- --ignored
rm "$sandbox_archive"
```

The second test imports the observed WASM export into fresh native memory/SQLite,
checks exact retained state, accepts a new native operation and reopens it. An old
pre-cutover checkpoint is never a rollback source. Whole-provider HTTP composition,
performance promotion, the gateway smoke flow and two observed stable release
cycles remain separate M8 gates. These fixtures do not authorize promotion or a
release.
