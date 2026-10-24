# Milestone 2 signed-shell integration

Task: SPEC-0013-M2-SHELL. Spec: `specs/0013-ink-v5-sandbox/spec.md` (APPROVED).
Branch: `feat/ink-v5-shell`. Worktree: `/tmp/kunveno-ink-v5-shell`.
Base: `db93db5` on `feat/ink-v5-worker` (PR #14, dependent on PR #13).
Primary writer: main implementation agent. Write scope: shared shell crate,
mock-provider integration/tests, workspace manifests/lock, isolated SDK worker,
policy, verification workflow/script and corresponding documentation.

## Owner decision

On 2026-10-22 the owner answered “yes, keep going” to the explicit request to
approve the reviewed SDK exceptions for provider-integration work. The exact
four maintenance advisories and thirteen package/version license exceptions are
now active for the standalone SDK gate, with the same December 1 expiry.
No extra exception, root policy waiver, authority switch, merge, release or
deployment is assumed. The registry versions and SDK cohort are unchanged.

## Implementation

`crates/provider-execution` is the single shell implementation used by both the
working Rust engine and the sandbox worker. It owns signature verification,
generation/expiry/nonce/replay admission, captured time/entropy, receipts and
events. Engines return effects or domain rejections. Storage owns atomic commit;
infrastructure failures never complete an accepted operation. Stale prepared
transitions cannot append events or replace receipts.

The shared shell verifies with the exact Schnorrkel 0.11.5 primitive and
`substrate` context used by the previous Subxt verifier. Both dependency graphs
already included it; no new registry package or cryptographic family was added.
Keeping the full signer out of the runtime avoids unused key derivation and an
additional unapproved license exception. App tests sign with Subxt and SDK
integration tests sign with sp-core, exercising compatibility independently.
Wallet key handling/signing and public signing bytes are unchanged.

The Rust provider's existing flat version-1 JSON and SQLite row are preserved.
Its engine still controls all app business state. Legacy checkpoints and old
receipts without captured inputs remain readable. The application root graph
adds only the local shell crate; it acquires no SDK packages.

The worker now accepts actual signed envelopes in an explicit Calendar fixture
mode. Clients cannot provide a success/failure shell blob for that mode. It
verifies before acquiring the SQLite writer lock, reads current state, admits
or replays through the shared shell, invokes WASM with the verified user as
caller and atomically saves runtime, shell, exact outcome and revision.
Domain/gas rejection preserves the business checkpoint and persists only failed
receipt/nonce; invalid envelopes, incomplete coverage and infrastructure errors
persist nothing. Replay returns before expiry/nonce/input capture, without a
WASM call or UPDATE. Queries still discard effects.

Fixture genesis pre-registers explicit funded accounts and stable calendar IDs.
The ABI bridge supports only whole-hour `SetCalendar` updates without overrides.
It decodes exact success/domain/language results and typed gas/trap failures;
unknown ABI/host failures reject as infrastructure errors. All other commands
or unsupported shapes return `IncompleteCoverage`. This is shell integration,
not M3 calendar or application parity. No new product rule is claimed.

The candidate SQLite schema identifies raw versus signed fixture mode. Wrong
mode, root, incompatible runtime/bundle or bad Calendar code rejects startup;
raw mutation admission cannot bypass the signed shell. Earlier prototype files
without the mode column fail rather than reseed. Those files were never app
provider databases; existing application data is unchanged.

## Verification and review

- SDK formatting/check/strict Clippy/build and all 23 tests pass: 8 unit,
  3 production-WASM compatibility, 6 signed-shell integration and 6 raw-worker
  integration tests. Existing child-trie, metering and shutdown regressions stay.
- Signed tests cover real caller identity, successful receipt/event IDs and
  cursor, original generation/entropy/receipt across restart, expired replay,
  invalid signature/expiry/generation/nonce, idempotency conflict, domain/gas
  rejection, unsupported operations, wrong mode/root, atomic SQL failure and
  two competing providers (one receipt/event for replay; one winner per nonce).
- `bash scripts/verify.sh ink-sandbox-dependencies`: passed advisories, bans,
  licenses and sources under the exact approved scope. A copied policy wrapper
  rejected an altered lock before running Cargo. Root `cargo deny --locked
  --offline check` still fails its documented advisory/license baseline;
  bans/sources pass. No additional finding was waived.
- Workflow actionlint passed in a read-only cached container with no network.
  Python bytecode compilation, shell syntax and `git diff --check` passed.
- An independent reviewer with xhigh reasoning approved M2 infrastructure at
  exact commit `65b9531`, subject to the full app gate. No actionable findings
  or code corrections. The reviewer independently ran 2 shared-shell unit tests,
  8 SDK unit tests and 6 signed integration tests: all passed. It confirmed
  verifier equivalence, legacy storage/envelope compatibility, failure atomicity,
  replay and exact policy scope. No unrelated full gates were rerun by reviewer.
- `bash scripts/verify.sh app`: passed at implementation commit `65b9531`.
  Workspace fmt/check/strict Clippy, 109 workspace tests, 2 doctests, isolated
  memory and SQLite suites, all builds, frontend WASM check and all 8 real-service
  E2E scenarios passed. Fresh temporary databases and cached PostgreSQL binaries
  were used. No browser API or signing/SCALE contract changes were needed.
- Nextest and cargo-audit remain unavailable as recorded in the first slice;
  the documented normal Cargo runner passed. No passing audit is claimed.

## Remaining scope

M2 infrastructure is complete at reviewed implementation `65b9531`: the full
application gate passed after the independent approval, satisfying its remaining
condition. The final follow-up records evidence only. This is not a release or
an authority promotion. M3–M8 still contain all business ports, full parity,
preserving import and current-state rollback. M0 performance acceptance remains
open until representative candidate domains exist; test duration is not p95
proof. No sandbox/shadow authority selector or production contract deployment is
introduced, and frontends keep the same API and behavior.
