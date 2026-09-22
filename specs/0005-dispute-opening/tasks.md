# Dispute opening: implementation plan

Status: READY FOR TECHNICAL REVIEW; implementation not started.

Updated: 2026-09-22. DSP-00 is documentation consolidation, not feature delivery.
Product scope is confirmed. Final compatibility/storage decisions are in spec.md.

## Ownership and scope

One integrator owns shared contracts, dispatch, module roots, OpenAPI and scripts.
Work sequentially in a single task worktree unless parallel work is explicitly
requested. Do not create worktrees inside .kilo. Tests belong in tests.rs modules.
No PostgreSQL migration, new service, DAO, chat or unrelated refactoring is
included in the proposed scope.

| ID | Work and primary paths | Depends on | Completion evidence |
|---|---|---|---|
| DSP-00 | Consolidate this spec, Disputas.md adenda, indexes and review handoff | Confirmed product decisions | Consistent spec, threat model and Gherkin; technical review resolved |
| DSP-01 | contracts submission/dispute modules and tests; generated-contracts/lib.rs; primitives only for stable validated hash types | DSP-00 | JSON/SCALE validation, payload compatibility, typed review/evidence contracts |
| DSP-02 | mock-provider domain submission/dispute modules and tests; State, project dispatch, validation, storage | DSP-01 | Atomic review/open/response; central freeze; restore, replay, races and conservation pass |
| DSP-03 | adapter dispute/submission handlers and tests; http/operations; provider internal resource query | DSP-01, DSP-02 | Authoritative target checks, exact public GET, authenticated writes, receipts/SSE |
| DSP-04 | OpenAPI, adapter/provider docs, existing Leptos completion controls and browser fixtures; custody version fixtures | DSP-03 | Public contract and all existing clients match; no old bypass route |
| DSP-05 | scoped tracing setup and typed errors in touched modules; manifests/lockfile only if necessary | DSP-02, DSP-03 | Structured output includes result IDs/cursors and excludes sensitive marker fixtures |
| DSP-06 | scripts/dispute-e2e.py; shared helpers from poc-e2e.py only when needed; existing regression payloads | DSP-04, DSP-05 | Signed real-service full flow on memory/SQLite; 5/3/4/2 regression still passes |
| DSP-07 | focused/full verification and progress/handoffs/DSP-07.md; spec/task statuses | DSP-06 | Observed requirements-to-tests evidence, actual limitations and clean diff |

DSP-01 freezes wire DTOs and stable errors first. DSP-02 adds failing domain
tests before transitions. DSP-03 connects those transitions through the existing
signed operation path. Response must not reuse an unrestricted project mutation
bypass. DSP-04 keeps the existing frontend working; it is not a full Figma UI.

## Specific implementation checks

- Accept/reject current submission ID; immutable review result and delivery.
- Refuse dispute without current rejection, after resubmission or across parents.
- Block every project command family and other milestones centrally.
- Exact replay returns prior receipt even after freeze; a new ID obeys guards.
- Allow only counterparty response and public case reads after freeze.
- No HTTP call to evidence URLs; no snapshot or new domain DB in adapter.
- Private provider queries remain service-authenticated.
- Restoration rejects inconsistent links, states, authors and versions.
- Existing payouts, committed-minute scores and reservations remain correct.
- Public DTO omits internal event recipients, operations, keys and unrelated data.
- Public GET exemption never exempts POST from session and CSRF checks.
- Tests use temporary state; incompatible user databases are never reset.
- Code uses explicit derived types, short domain functions and dedicated tests.rs.

## Test traceability

| Requirements | Evidence |
|---|---|
| REF-001, SUB-001, SUB-002 | Contract validation; submit/current-review tests; existing acceptance regression |
| REJ-001, REJ-002, DSP-001 | Rejection/resubmission and opening authorization/resource ancestry tests |
| DSP-002, DSP-003 | Atomic freeze, all command guards, races, conservation and restore tests |
| DSP-004, DSP-005 | Unique response, anonymous read and protected write tests |
| DSP-006 | Receipt replay/lost response, SSE reconnect, backend equivalence |
| DSP-007 | Absent-resolution contract and denied root/principal mutation tests |
| Full flow | New public HTTP E2E plus unchanged four-milestone 5/3/4/2 outcomes |

## Verification and handoff

Run focused tests, then formatting, workspace check/clippy/tests/doc tests/build
with applicable feature combinations. Run the signed E2E on both backends.
Omit cargo-deny per the owner's instruction; do not claim that gate passed.
Do not close unrelated TASK-005/POC-07 without their separate required evidence.

The final handoff records exact commands, observed results, branch/base/final
commits, changed paths, Gherkin mapping and any remaining gaps. Public API E2E
does not establish browser dispute UX, real-chain compatibility or DAO readiness.
