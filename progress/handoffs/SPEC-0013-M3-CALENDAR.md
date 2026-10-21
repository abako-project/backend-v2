# SPEC-0013 M3: Calendar slice

Task: implement and review the Calendar part of approved SPEC-0013 M3.
M3 remains in progress until Directory/catalog/registration/mode parity and its
signed fixture bridge are complete. Rust remains the sole application authority.

Backend branch: `feat/ink-v5-directory-calendar`, worktree
`/tmp/kunveno-ink-v5-directory-calendar`, based on reviewed M2 `9900cfe`.
Canonical branch: `feat/directory-calendar-v5`, worktree
`/tmp/kunveno-contracts-directory-calendar`, based on `e3fa4c4`.
Write scope: new canonical `calendar-v5/` and target exclusion; pinned backend
artifact/lock/manifest, sandbox bundle list/tests/docs, Rust Calendar tests and
shared test fixtures, SDK CI path filter, and this progress record. No public
contracts, normal app dependencies, business implementation or live data change.

## Implemented

- Canonical source `4687748`: one v5 Calendar instance per worker. Integer-minute
  defaults and overrides; exact Chrono ISO-week validation and progression;
  checked weekly totals; earliest complete allocation within an inclusive window;
  all-or-nothing reservation; capacity updates retain all reservation references.
- Immutable creating Directory, worker owner and reservation-controller
  capabilities. Only owner/Directory changes capacity; only controller reserves.
  Fixture accounts exercise these roles. Project/controller authorization and
  atomic multi-calendar approval remain M5/M6, not a claimed browser permission.
- Existing legacy source/artifacts unchanged. Backend pins the new source,
  release WASM, ABI and build lock; no startup compilation or node.
- One committed fixture set independently checked against real WASM and the
  existing Rust Calendar. Cases cover week 53/year changes, year zero, zero and
  sparse capacity, full u32 amounts, capacities above a physical week, exact
  reference retention, no-op zero requests, overcommit and duplicate overrides.
- Direct WASM checks cover denied callers, mutating dry-run isolation, invalid
  ISO inputs/reversed windows, gas exhaustion, concurrent reservations and restart.

No new reservation-release rule is invented. Completion retains consumed hours;
cancellation/dispute has no agreed release behavior (SPEC-0003 CAL-002).
Calendar's SCALE layout is checked using the existing generated provider DTOs;
no SDK or ink dependency enters the app workspace.

## Verification and review

Source build: `cargo +1.88.0 contract build --release --offline --manifest-path
calendar-v5/Cargo.toml`; formatting and strict native Clippy passed. The new
Chrono direct use matches the app's version, disables clock/time-zone/native JS
features and is reviewed in canonical `calendar-v5/README.md`.

Observed so far: native shared-case test passes; three initial WASM tests pass.
Full app, full SDK (including added concurrency test), exact-lock SDK policy,
fresh-source rebuild and independent high-reasoning review are being collected.
This record must be updated with final results before the slice is handed off.

## Compatibility and remaining work

The new artifact changes the candidate cohort identity. Old private fixture DBs
are rejected rather than reinterpreted; create a fresh disposable candidate DB.
The normal mock's persisted JSON/SQLite and public frontend behavior are untouched.
No production/default provider switch, hybrid authority or automatic fallback.

Next M3 slice: Directory-owned catalogs and descriptive skill/role associations,
registration with atomic Calendar creation, qualifications, modes/promotion,
skill requests, root catalog edits/deletion guards and signed shared-shell
fixtures. Existing app rules, generated IDs and event receipts must match Rust.
Full proposal references need Project wiring in M5; never report incomplete
domain coverage as successful full parity.

M0 representative performance measurements, large-state import suitability,
M4-M8 and promotion/release approval remain separate gates. No release, deployment,
master merge or tag is performed by this task. Root dependency-policy findings
remain the known baseline; the isolated SDK approval does not waive them.
