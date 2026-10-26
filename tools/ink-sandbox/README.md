# Embedded contract provider

This package runs the canonical Directory, Calendar and Project contracts as
ink! 5.1.1 WASM in one FRAME/pallet-contracts runtime. No parachain nodes or block
production are needed. It uses the same internal HTTP boundary and signing shell
as the native mock; browser clients continue using the adapter's REST/SSE API.

The SDK has its own Cargo workspace and lockfile. Normal application builds keep
the native provider and do not acquire the SDK dependency graph. The candidate
is functional but is not deployed or approved for promotion; see
[dependency policy](policy/README.md) and [current limits](../../docs/status.md).

## Build and run

Use the repository's Rust toolchain. From the repository root:

```sh
cargo build --release --manifest-path tools/ink-sandbox/Cargo.toml --locked --bin whole-provider
bash scripts/verify.sh ink-sandbox
bash scripts/verify.sh ink-sandbox-dependencies
```

If `/tmp` uses RAM, set `TMPDIR` and `CARGO_TARGET_DIR` to directories on disk.
Use the existing local secret files; do not put secret values in shell arguments.
Configure `MOCK_ROOT_ACCOUNT_FILE`, `INTERNAL_SERVICE_TOKEN_FILE`, `BIND_ADDR`,
`WHOLE_DATABASE_PATH` and `PROVIDER_AUTHORITY=rust|shadow|sandbox`, then run
`whole-provider serve`. With no command, it serves. An empty destination gets
explicit development genesis; an existing incompatible store fails startup.

| Authority | Business result | Comparison |
|---|---|---|
| `rust` | Native reference | Disabled |
| `shadow` | Native reference | Execute and compare WASM independently |
| `sandbox` | WASM contracts | Compare against the native reference |

Authority is explicit and persisted. Comparison never silently substitutes an
engine. `whole-provider diagnostics` reports the selected authority and complete
comparison status. Raw fixture calls and test controllers are not public APIs.
The native reference remains until the approved promotion/release gates are met.

## Where to read the code

| Path | Purpose |
|---|---|
| [src/lib.rs](src/lib.rs), [assets.rs](src/assets.rs) | Runtime, unchanged resource caps and asset extension |
| [src/worker.rs](src/worker.rs), [checkpoint.rs](src/checkpoint.rs) | Bounded worker queue, persistence, shutdown and complete child-trie checkpoints |
| [src/worker/whole.rs](src/worker/whole.rs) | Authority selection and atomic comparison/commit |
| [src/worker/directory.rs](src/worker/directory.rs), [projects.rs](src/worker/projects.rs) | Private signed ABI routing |
| [src/worker/archive.rs](src/worker/archive.rs), [project_storage.rs](src/worker/project_storage.rs), [migration.rs](src/worker/migration.rs) | Ordinary ink cells, complete exports and fresh sealed imports |
| [src/provider.rs](src/provider.rs), [src/bin/whole-provider.rs](src/bin/whole-provider.rs) | Existing provider interface and owner CLI |
| [fixtures/manifest.json](fixtures/manifest.json) | Exact source, WASM/ABI and build-lock pins |
| `tests/` | Signed parity, financial failure recovery, growth, import/restart, HTTP and rollback |

The bundle contains three application contracts and one small private
`sandbox-probe` for traps, nested reverts, gas exhaustion and persistence tests.
Application rules live in the [canonical contracts repository](https://github.com/abako-dev/contracts/tree/review/ink-v5-runtime).
The worker owns transport execution, not another copy of contract business logic.

## Persistence and boundaries

The shared shell verifies signatures, generation, nonce and idempotency. Each
accepted execution captures its time and entropy once. Runtime effects, receipts,
events and captured inputs commit together in SQLite. Failed database commits
restore the previous checkpoint; exact retries preserve the original outcome.
The worker queue is bounded at eight requests and shutdown observes in-flight work.

Contracts use ordinary ink storage. All growing lists have individually addressed
records; there is no custom paging library. Project operations still traverse
history, so memory/gas remain finite. Existing caps include 16 KiB storage cells,
192 KiB contract code, 16 WASM memory pages and 64 MiB archive/checkpoint budgets.
No resource cap was increased for the simplification.

The adapter owns descriptive content, URL validation, task text/progress and
notifications. Contracts retain numeric terms, exact money, permissions, task IDs
and assignees. Evidence uses fixed commitments. Required legacy URL preimages stay
offchain for original signed replay and complete rollback; no URL parser runs
inside the contracts. Legacy signed replay and metadata lowering remain supported;
fresh legacy task-content writes are rejected because the API now owns those edits.
[Architecture](../../docs/architecture.md) describes ownership.

## Import, backup and rollback

Pause writes and drain/reconcile uncertain operations before changing authority or
artifact cohort. Keep the configured root and provider generation. Preserve API,
wallet and provider backups together; a provider archive is not a complete app backup.
Use protected new file paths. The CLI refuses existing import/output destinations.

```sh
# Source stopped; WHOLE_DATABASE_PATH identifies its store.
whole-provider export current.json
# Use export-native instead when the source is native mock SQLite.

# Set WHOLE_DATABASE_PATH to a fresh destination and choose its authority.
whole-provider import current.json
whole-provider diagnostics
whole-provider export reopened.json
```

Compare the complete decoded archives before switching routing. Imports validate
actual IDs, funds, capacity, history and replay state, then create dormant contracts
using bounded typed batches. Contracts seal only after exact comparison; imports
do not replay payments. A failure cannot publish a partial destination.

Changes to the runtime or pinned bundle identity require fresh import. Renamed
components do not reinterpret an existing physical checkpoint. Rollback exports
**current** state after accepted writes into the frozen compatible predecessor;
never restore a stale pre-cutover checkpoint. The explicit
[task ownership procedure](../../docs/operations/task-cutover.md) handles paired
API/core changes. Signed and archive format versions remain intentional.

## Reproduce and verify

The rebuild tool archives the manifest's source commit from a canonical checkout,
uses its exact build locks, and compares generated WASM and ABI. It publishes nothing:

```sh
python3 tools/ink-sandbox/rebuild.py /path/to/contracts --offline
```

The normal SDK gate checks formatting, compilation, strict Clippy, tests and build.
The signed suites compare complete facts, receipts and histories with the native
engine, including failure, restart, growth and reordered/shrunk collections.

The frozen-predecessor test needs an independently preserved executable and hash:

```sh
export INK_STORAGE_PREDECESSOR=/protected/preceding/whole-provider
export INK_STORAGE_PREDECESSOR_SHA256=RECORDED_SHA256
cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked --test storage_cohort -- --ignored
```

It verifies current writes, predecessor import/reopen/replay and further work,
incompatible checkpoint rejection, then fresh roll-forward. Its normal ignored
status does not count as a passing rollback check.

For the real wallet/adapter/PostgreSQL flows, first build the application binaries:

```sh
python3 scripts/poc-e2e.py --storage sqlite \
  --provider-binary tools/ink-sandbox/target/release/whole-provider \
  --provider-authority sandbox
```

Repeat for `rust` and `shadow`; matched comparisons are checked. Browser/frontend
deployment requires its own walkthrough. Controlled engine timing is opt-in:

```sh
cargo test --release --manifest-path tools/ink-sandbox/Cargo.toml --locked \
  --test performance whole_provider_engine_timings -- --ignored --nocapture
```

Use an idle controlled host and record its resources/profile. A functional test
pass is not latency acceptance. Current evidence belongs in the
[review PR](https://github.com/abako-project/backend-v2/pull/24); historical milestone
records remain in Git, not in the current setup instructions.
