# ink! v5 compatibility proof

This is milestone 0 of [SPEC-0013](../../specs/0013-ink-v5-sandbox/spec.md).
It runs production WASM in a native FRAME runtime without starting a node. It
is deliberately a separate Cargo workspace and lockfile: the application keeps
its current dependencies, authoritative Rust engine and startup behavior.

```sh
bash scripts/verify.sh ink-sandbox
```

The proof executes Calendar → Ratings and Project → Calendar calls, direct
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
unchanged. Directory and signed candidate routing are the next M3 slice; the
existing signed Calendar fixture still tests the legacy contract.

Directory will create one Calendar instance per worker. Only the worker or its
creating Directory can change capacity; only an immutable reservation controller
can reserve. These tests pass explicit fixture accounts as those capabilities.
They do not implement Project permissions or expose a new provider/browser API.
No calendar release operation is added: the approved rules retain consumed hours.

Adding the bundle changes the pinned candidate cohort identity. Existing private
candidate databases fail closed; use a new disposable fixture database. Ordinary
mock-provider SQLite data and its Rust authority remain unchanged. Large-state
import, performance budgets and complete M3 parity remain open. See the
[slice handoff](../../progress/handoffs/SPEC-0013-M3-CALENDAR.md).

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
application permits spending the full visible balance. Candidate contracts and
the bridge must solve this through the approved separate technical-reserve
policy before money parity can pass; silently reducing user payouts is forbidden.

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
claim Directory/Calendar parity, which remains M3. There is no public raw-contract
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

The candidate SQLite schema now distinguishes raw and signed fixture modes.
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
