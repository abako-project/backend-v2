# Development plan: dispute opening

Status: BLOCKED ON SPEC APPROVAL

## Graph rules

- No implementation starts while `status.md` is `DRAFT`.
- Every task uses its named branch/worktree, one primary writer and only its
  allowed paths.
- Tests are kept in dedicated `tests.rs` files, not embedded in `lib.rs`,
  `domain.rs` or handler modules.
- The integrator alone edits shared manifests, module roots, generated aggregate
  dispatch, OpenAPI and lockfiles.
- A task stops rather than guessing an unanswered blocking rule in `status.md`.

## Tasks

| ID | Branch / worktree name | Primary scope | Depends on | Completion gate |
|---|---|---|---|---|
| DSP-00 | `spec/0005-dispute-opening` / `dispute-spec` | `specs/0005-dispute-opening/**` | Product answers | Spec and scenarios approved |
| DSP-01 | `feat/dispute-contracts` / `dispute-contracts` | `crates/generated-contracts/src/disputes/mod.rs`, `crates/generated-contracts/src/disputes/tests.rs` | DSP-00 | Validated DTOs, signed commands, events, bounds, errors and compatibility fixtures pass |
| DSP-02 | `feat/dispute-provider-domain` / `dispute-provider-domain` | `services/mock-provider/src/disputes/mod.rs`, `services/mock-provider/src/disputes/tests.rs` | DSP-01 | Pure rejection/open/append transitions, evidence capture, invariants and rollback pass |
| DSP-03 | `feat/dispute-adapter` / `dispute-adapter` | `services/adapter-api/src/disputes/mod.rs`, `services/adapter-api/src/disputes/tests.rs` | DSP-01 | Session authorization, REST mapping, redaction and query projections pass against fixtures |
| DSP-04 | `feat/dispute-frontend` / `dispute-frontend` | `apps/leptos-web/src/disputes/mod.rs`, `apps/leptos-web/src/disputes/tests.rs` | DSP-01 | Rejection/open/response and read-only public UI use only REST/SSE |
| DSP-05 | `integrate/dispute-opening` / `dispute-integration` | `crates/generated-contracts/src/lib.rs`, provider `domain.rs`/`lib.rs`/`storage.rs`, adapter `http.rs`/`lib.rs`/`operations.rs`, `contracts/openapi.json`, root manifests and lockfile | DSP-02, DSP-03 | Commands commit atomically in memory and SQLite; old route cannot bypass rejection |
| DSP-06 | `test/dispute-opening-e2e` / `dispute-e2e` | `scripts/dispute-e2e.py`, `progress/handoffs/DSP-06.md` | DSP-04, DSP-05 | Signed rejection/open/response, SSE, redaction, replay and frozen escrow pass on both backends |
| DSP-07 | `verify/dispute-opening` / `dispute-verification` | Read-only integrated tree, `progress/handoffs/DSP-07.md` | DSP-06 | Requirement-to-scenario trace and applicable workspace gates independently verified |

DSP-01 may require a minimal integrator-owned re-export from
`crates/generated-contracts/src/lib.rs`; the contracts writer does not edit that
shared file. DSP-02 and DSP-03 prepare self-contained modules. DSP-05 performs the
only edits to the current large provider dispatch and adapter route table.
The process-level E2E remains a standalone Python fixture; every new Rust test is
placed in the `tests.rs` files named above.

## Parallel-safe execution

After DSP-01 freezes fixtures, DSP-02 and DSP-03 can run in parallel. DSP-04 can
build against those fixtures without accessing provider or custody internals.
DSP-05 starts only when provider and adapter modules are ready. DSP-06 and DSP-07
remain sequential because they consume the integrated behavior.

## Requirement traceability

| Requirement | Implementation tasks | Acceptance evidence |
|---|---|---|
| REJ-001, REJ-002 | DSP-01, DSP-02, DSP-03 | Rejection validation, authorization and optional-dispute scenarios |
| DSP-001, DSP-002 | DSP-01, DSP-02, DSP-05 | Opening authorization, atomic success and rollback scenarios |
| DSP-003 | DSP-02, DSP-05 | Immutable evidence scenario and restoration tests |
| DSP-004 | DSP-01, DSP-02, DSP-03 | Counterparty append and impersonation scenarios |
| DSP-005 | DSP-01, DSP-02, DSP-04 | Linked-channel scenario plus approved channel behavior |
| DSP-006 | DSP-01, DSP-03, DSP-04 | Public allowlist and redaction tests |
| DSP-007 | DSP-02, DSP-05 | Escrow conservation and absent-resolution scenarios |
| DSP-008 | DSP-02, DSP-03, DSP-06 | Provider event and authenticated SSE scenario |
| DSP-009 | DSP-02, DSP-05, DSP-06 | Replay/lost-response and memory/SQLite scenarios |

## Integration order

1. Approve the state, visibility, channel and evidence decisions.
2. Freeze versioned contracts and compatibility behavior.
3. Integrate provider state transitions and storage validation.
4. Integrate adapter commands, queries and OpenAPI.
5. Integrate the independent frontend flow.
6. Run E2E on memory and SQLite, then independent verification.

## Applicable gates

Focused tests run first, followed by formatting, workspace check, clippy, tests,
doc tests, build, dependency policy and audit gates defined by the repository.
Contract and provider tests must prove that no normal completion or replay can
release disputed funds.
