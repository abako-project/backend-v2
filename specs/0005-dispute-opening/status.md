# Specification status

State: APPROVED

Updated: 2026-09-23

Implementation: COMPLETE at `a64b747`. Verification: PASSED for
the approved memory/SQLite mock scope. PostgreSQL and dispute resolution remain
separate work.

## Delivered

- Versioned milestone submissions with recorded HTTPS URL references (content hash removed with owner approval on 2026-10-09).
- Current-submission acceptance and rejection; rejection enters ChangesRequested.
- Open public dispute entity, one active case per project and project-wide freeze.
- One immutable counterparty response and anonymous public case read.
- Adapter REST/auth/CSRF boundary, custody-signed payload version 2 and SSE events.
- Leptos forms for delivery, rejection and opening; existing completion routes remain.
- Central provider guards, state restoration validation and structured tracing.

The old permissive milestone dispute route and DisputeMilestone command are
removed. Rejection does not implicitly open a dispute. Opening and response do
not move funds, scores or reservations. No resolution or unlock operation exists.

## Observed evidence

- Contract/OpenAPI tests: 11 passed.
- Provider unit/integration tests: 18 passed, including memory and SQLite dispute
  races, replay, restore, stale submission, freeze and response rules.
- Custody tests: 9 passed with payload version 2.
- Clippy with all targets/features and WASM frontend check passed.
- Signed real-service happy path passed on memory and SQLite.
- Signed four-milestone regression passed with teams 5/3/4/2 on both backends.
- Signed dispute E2E passed on both backends through public response.
- Local cargo-audit reported no vulnerabilities and only the two accepted
  unmaintained warnings inherited through the Leptos dependency tree.

Exact final commands and results are in `../../progress/handoffs/DSP-07.md`.
Cargo-deny is intentionally omitted by owner instruction.

## Deferred by specification

DAO adjudication, resolution/unlock, timeout disputes, due_at extensions, chat,
encrypted communication, evidence retention and PostgreSQL migration.
