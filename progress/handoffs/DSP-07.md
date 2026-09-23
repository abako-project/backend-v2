# DSP-07 handoff

Date: 2026-09-23
Branch: `feat/dispute-opening`
Base commit: `f597a2f7a5b085dd80f79dafd7acc22ec058e761`
Specification: `specs/0005-dispute-opening`

## Delivered

- Versioned completion submissions and public evidence references.
- Current-submission rejection and acceptance with unchanged settlement/scoring.
- Provider-owned Open dispute, atomic project freeze and one counterparty response.
- Public case projection; authenticated/CSRF-protected writes.
- Custody payload version 2, provider/adapter tracing and OpenAPI contracts.
- Leptos completion, rejection and opening controls.
- Signed real-service dispute scenario shared by the existing E2E harness.

The compatibility routes `request-completion` and `accept-completion` remain.
The old milestone `/dispute` route and unrestricted `DisputeMilestone` command
are removed.

## Verification observed

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets --all-features --locked --offline`: passed.
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`: passed.
- `cargo test --workspace --all-features --offline`: 49 passed, 0 failed.
- `cargo check -p leptos-web --target wasm32-unknown-unknown --all-features --offline`: passed.
- `cargo build --workspace --all-targets --all-features --locked --offline`: passed.
- `cargo test --workspace --doc --offline`: passed.
- `cargo audit --no-fetch`: passed with the two previously accepted
  unmaintained warnings from the Leptos tree; no vulnerabilities reported.
- `python3 scripts/poc-e2e.py --storage both`: happy path and four-milestone
  5/3/4/2 regression passed on SQLite and memory.
- The `exercise_dispute` scenario passed on SQLite and memory through public
  counterparty response, with project mutation denied and balances conserved.
- codebase-memory reindex completed with 2,755 nodes and 7,505 edges; all six
  new dispute module/test paths report no recorded coverage issue.

Focused provider coverage includes stale reviews, rejection/resubmission,
unauthorized and concurrent openings, receipt replay, all project command
families under freeze, unique response and SQLite restoration.

## Decisions

Provider/mock remains the business source of truth. Adapter holds transport,
sessions and notification read state only. Evidence URLs are validated but never
fetched. Payload version 2 intentionally rejects old encoded commands and the
disposable SQLite schema now requires version 2.

No worktree was created for this implementation. An unrelated clean detached
worktree was removed; future isolated worktrees belong under `/tmp`.

## Deferred and risks

This PoC has no dispute resolution, unlock, DAO adjudication, timeout, due_at
extension, encrypted chat, evidence retention guarantee or PostgreSQL migration.
A public hash proves equality with retrieved bytes, not availability or quality.
Full Figma dispute-management UX remains separate from these functional controls.

Cargo-deny was omitted by owner instruction. TASK-005 and POC-07 are not closed
by this handoff because their remaining repository-wide criteria are separate.
