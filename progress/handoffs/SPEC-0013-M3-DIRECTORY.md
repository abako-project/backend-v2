# SPEC-0013 M3: Directory slice

Task: port Directory business rules under approved SPEC-0013, SPEC-0003 and
SPEC-0006. M3 remains in progress until the real signed Directory/Calendar fixture
bridge is complete. Rust remains the sole application authority.

Backend branch `feat/ink-v5-directory`, worktree `/tmp/kunveno-ink-v5-directory`,
base `b375b07` (reviewed Calendar slice). Canonical branch `feat/directory-v5`,
worktree `/tmp/kunveno-contracts-directory`, base `4687748`.

Write scope: canonical `directory-v5/`; backend pinned artifact/build-lock/manifest,
sandbox bundle/test/docs, test-only native trace harness and fixture, SDK workflow
paths, and this handoff. No app business implementation, public contract, dependency
lock, provider selection, authentication or live data changes. The Rust test module
is nested under execution so its production RNG API remains private and unchanged.

## Implemented

Canonical implementation `0c35671` adds:

- Immutable root authority; explicit catalog seed (9 roles/33 skills in fixtures),
  fixed coordinator role 1, descriptive associations and exact skill normalization.
- Atomic worker/Calendar creation using typed v5 cross-calls. Calendar public IDs
  remain unique, registration starts in Worker mode with zero fresh reputation,
  qualifications reference current catalogs, and eligibility never self-promotes.
- Worker views read the actual Calendar. Updates/mode changes preserve reservations;
  nested overcommit errors propagate without partial state.
- Root catalog writes/deletion guards, skill-request visibility, approval/rejection,
  duplicate/name/role rechecks, checked skill-ID allocation and metadata preservation.
- A fixed internal reference-controller capability for all retained proposal
  qualifications. Explicit fixtures exercise it; M5 must integrate real Project
  lifecycle changes before project-backed deletion parity is claimed.
- No new registry package/version beyond Calendar's existing graph, no handwritten
  unsafe or new native/crypto dependency, no asset operation or visible user fee.
  Runtime fixtures fund native execution/storage separately from KVN assets.

The shared 48-step fixture freezes captured IDs, domain effects/errors and SHA256
of SCALE `(roles, skills, associations, workers, requests)` after every transition.
Rust and real WASM replay it independently; the VM tests do not link the Rust engine.
Direct WASM tests check requester/root visibility, reference guards and denied
controllers, actual Directory-to-Calendar caller semantics, parent/child rollback,
unique IDs and full raw-trie restart mid-trace.

Trace authoring is an explicit ignored unit test, not part of normal verification:
set `DIRECTORY_TRACE_OUTPUT` to a temporary path and run `cargo test -p mock-provider
--lib export_directory_trace --all-features --locked -- --ignored`. Review before
replacing the committed fixture; ordinary tests only read it.

## Unicode compatibility

Measured stdlib versions: app Rust 1.96.1 uses Unicode 17, v5 compiler Rust 1.88.0
uses Unicode 16. There are 28 changed character lowercase mappings, plus sigma
context-property changes. New compiler release builds fail because cargo-contract
5.0.3 still requests the removed `panic_immediate_abort` feature.

The working v5 compiler/tool remains unchanged. Canonical `names.rs` combines the
standard character mapping with a generated v16/v17 delta and Unicode 17 Cased /
Case_Ignorable ranges. Official versioned UCD sources, hashes, generator and Unicode
license notice are committed. Compile-time version guards prevent silent drift.

Exhaustive cross-toolchain output comparison covers 3,336,192 scalar/sigma cases
plus whitespace classification for every Unicode scalar. Rust 1.88 compatibility
and Rust 1.96 reference streams have identical SHA256
`a807122c9e20dfa9b4cc079e2eaa0d18341ecf3c642621b7338d09eacb138c38`.
The additional native semantic unit test passes against the app's Unicode 17.
The 48-step WASM trace includes Unicode 17-only case pairs and sigma context.

## Verification and review

Canonical source build, formatting and strict Clippy pass. Full SDK gate passes
(31 tests: prior 27 plus four Directory tests). Native frozen-trace replay passes;
exhaustive Unicode tests/comparison pass. Full application gate, fresh-source
artifact reproduction, unchanged exact-lock SDK policy, actionlint and the
independent high-reasoning review are being collected. Record final revision,
results and verdict here before handoff. Local logs use `/tmp/ink-directory-*.log`.

## Remaining gates

Next M3 slice: actual signed Directory fixture integration with the shared shell;
same verified caller and captured inputs; exact failed/success receipts, nonces,
event cursors and atomic SQLite runtime/shell commit; invalid envelopes, replay,
incomplete coverage, restart and competing workers. Current traces compare domain
effects/projections, not signed-operation admission or durable shell history.

New bundle membership changes the candidate cohort identity, so old private fixture
databases reject startup; use a fresh disposable fixture DB. Ordinary mock SQLite
and frontend behavior are untouched. Do not enable hybrid authority or fallback.
M4 reputation/matching, M5 actual Project reference wiring, M0 performance,
large-state import capacity and M6-M8 remain open. Root dependency-policy baseline
findings remain unchanged and unwaived. No master merge, tag, image publication or
deployment is performed by this task.
