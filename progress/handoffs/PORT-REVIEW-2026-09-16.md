# Port review: 2026-09-16

## Scope and result

Compared the integrated Rust marketplace against legacy `main` at `3e2b929`,
including provider-owned proposal task storages, using both indexed code graphs
and direct source for exact contract details. The Rust marketplace implements
the approved redesign; it is not a full wire or behavior parity port. The
functional comparison and remaining work are in
[`docs/project/porting-coverage.md`](../../docs/project/porting-coverage.md).

The legacy-scale E2E, test extraction, mock-domain split, mock integration-test
split and draft dispute-opening plan are integrated into `master`. The dispute
plan is not an approved implementation.

## Verification on the integrated tree

| Command | Observed result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo check --workspace --all-targets --all-features --locked` | Pass |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Pass |
| `cargo nextest run --workspace --all-features` | 46/46 pass with local socket access; sandbox-only socket denial on first attempt |
| `cargo test --workspace --doc` | Pass; no doc-test cases |
| `cargo test -p mock-provider --no-default-features --features storage-memory,mock-seed --locked` | 8 tests pass |
| `cargo test -p mock-provider --no-default-features --features storage-sqlite,mock-seed --locked` | 11 tests pass |
| `cargo build --workspace --all-targets --all-features --locked` | Pass |
| `cargo check -p leptos-web --target wasm32-unknown-unknown --locked` | Pass |
| `python3 scripts/poc-e2e.py` | Pass: original and four-milestone scenarios on SQLite and memory |
| `cargo audit` | Exit 0; two unmaintained dependency warnings |
| `cargo deny check` | Exit 5; CC0/Boost policy and two Leptos maintenance advisories |

The first mock-domain branch builds printed an intermittent Rust compiler
SIGSEGV backtrace, but later targeted tests, Clippy and all integrated compiler
gates completed successfully. No application failure was observed in the
integrated runs. Revisit if this recurs.

## Open decisions and delivery boundaries

- Task storage: whether an empty storage blocks proposal submission. Legacy
  requires one task per milestone; SPEC-0003 and Rust do not.
- Marketplace lifecycle: sequential milestone activation, assignment-key
  continuity and an explicit completed-project state are not ported.
- SPEC-0004 Virto/Bramp/governance remains draft and unimplemented.
- SPEC-0005 formal dispute opening remains draft. Current freeze/reason command
  is not the documented rejection/expediente/channel/publication flow.
- TASK-005 and POC-07 independent acceptance/security closure remain open,
  including secret-marker diagnostic evidence and key-rotation scope.
- Cargo dependency-policy exceptions approved in principle for the PoC have
  not been encoded, so the `cargo deny` gate remains red. No production release
  or real-chain provider is implied by the passing local E2E.
