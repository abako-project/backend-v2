# Embedded contract provider

This package runs Dao and Project as ink! 5.1.1 WASM in one
FRAME/pallet-contracts runtime. Local development needs no parachain nodes or
block production. Frontends continue using the adapter’s REST/JSON and SSE API.

The SDK has a separate Cargo workspace and lockfile. The standalone native
provider remains the normal application build until the SDK dependency policy
passes. It is not executed inside this runtime. See [policy](policy/README.md)
and [current limits](../../docs/status.md).

## Build and run

From the repository root, using the pinned Rust toolchain:

```sh
cargo build --release --manifest-path services/contract-provider/Cargo.toml --locked --bin contract-provider
bash scripts/verify.sh contract-provider
bash scripts/verify.sh contract-provider-dependencies
```

Set `TMPDIR` and `CARGO_TARGET_DIR` to disk-backed directories if `/tmp` uses RAM.
Configure `MOCK_ROOT_ACCOUNT_FILE`, `INTERNAL_SERVICE_TOKEN_FILE`, `BIND_ADDR` and
`CONTRACT_DATABASE_PATH`. Set `ENABLE_MOCK_FUNDING=true` for development funding.
Run `contract-provider serve` (or run it without arguments).

A new database starts the contracts through their ordinary constructors with a
small catalog seed. Existing compatible state reopens; incompatible, empty or
corrupt existing stores fail startup. There is no authority selector, archive
import/export, migration or automatic reset. Development starts with a clean
matching API, wallet and provider environment.

## Where to read the code

| Path | Responsibility |
|---|---|
| [src/lib.rs](src/lib.rs), [assets.rs](src/assets.rs) | Runtime resource limits and asset extension |
| [src/worker.rs](src/worker.rs), [checkpoint.rs](src/checkpoint.rs) | Bounded queue, atomic persistence and shutdown |
| [configuration.rs](src/worker/configuration.rs) | Fresh constructor-based startup |
| [dao.rs](src/worker/dao.rs), [projects.rs](src/worker/projects.rs) | Signed command routing into WASM |
| [reads.rs](src/worker/reads.rs), [facts.rs](src/worker/facts.rs), [project_storage.rs](src/worker/project_storage.rs) | Current ink storage reads and private API views |
| [src/provider.rs](src/provider.rs) | Existing private HTTP provider interface |
| [fixtures/manifest.json](fixtures/manifest.json) | Source, WASM, ABI and build-lock pins |

The normal bundle contains only Dao and Project. The private `sandbox-probe`
lives under `tests/fixtures/`, with a separate manifest, and is loaded into the
worker only under `cfg(test)`. Its worker regressions live in
[src/worker/persistence_tests.rs](src/worker/persistence_tests.rs). Probe changes do
not change the application bundle identity. Test checkpoints have a distinct
identity and cannot reopen as application state. Removing the probe from the
application manifest changes its bundle identity once; older checkpoints are
rejected and never silently reset. Business rules live in the
[canonical contracts](https://github.com/abako-dev/contracts/tree/review/ink-v5-runtime).
The worker adds transport execution and persistence, not another business engine.
Shared execution/HTTP code comes from `provider`; production builds do not depend
on `native-provider`. The retained golden-trace tests use native conversion helpers.

## Boundaries and persistence

The shared shell verifies signatures, generation, nonce and idempotency. Runtime
effects, receipts, events and execution inputs commit together in SQLite. Failed
commits leave the previous checkpoint intact. Exact replay returns the original
receipt. The queue holds at most eight requests; shutdown observes active work.
Shell-only reads do not decode the WASM checkpoint. Snapshot reads assemble
current contract cells directly.

Dao groups ordinary ink storage into config, catalog, members, projects,
calendars and bank. The facts reader follows the pinned scalar encoding and
explicit mapping keys; a changed layout requires matching artifacts and reader.

The adapter owns descriptive content, URL validation, task text/progress and
notifications. Contracts retain permissions, numeric terms, exact money, task IDs
and assignees. Evidence uses fixed commitments. Growing records use ordinary ink
`Lazy`, `Mapping` and `StorageVec`; there is no custom paging layer or onchain URL
parser. Limits remain 16 KiB per storage cell, 192 KiB contract code, 16 WASM memory
pages and 64 MiB checkpoint/shell blobs. Whole-history operations still have finite
memory and gas costs.

Preserve matching API, wallet and provider backups together with their secrets.
A provider checkpoint cannot reconstruct API-owned content. Stop/reconcile writes
before backups or replacement; never mix generations or silently reopen an old
checkpoint using different artifacts.

## Verification

Reproduce the pinned artifacts from a committed contracts checkout. Use a disk-backed
`TMPDIR` outside the backend Cargo workspace for this independent build:

```sh
python3 services/contract-provider/rebuild.py /path/to/contracts --offline
python3 services/contract-provider/rebuild.py /path/to/contracts --offline --test-fixtures
```

After building the application binaries, run the real adapter/wallet/PostgreSQL
scenarios against the embedded provider:

```sh
python3 scripts/poc-e2e.py --storage sqlite \
  --provider-binary services/contract-provider/target/release/contract-provider
```

Current signed tests start from constructors and check money, membership,
permissions, replay, failure recovery and restart. Functional verification is not
controlled-host latency acceptance or evidence of a deployed browser cutover.
Record commands, tested revisions and limitations in the
[review PR](https://github.com/abako-project/backend-v2/pull/24).
