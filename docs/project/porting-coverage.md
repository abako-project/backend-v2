# Porting and E2E coverage

Reviewed 2026-09-16 against Rust `master` and legacy `main` commit `3e2b929`.
This is a port of the approved marketplace redesign,
not a complete compatibility port of the legacy backend or all mock endpoints.

## Source baseline

Legacy `main` now includes `feat/68-provider-owned-proposals-task-storages` through
merge commit `3e2b929`; the feature implementation is commit `d408641`. The Rust
repository remains separate. Legacy `/v1` clients need adaptation to `/api`.

## Functional comparison

| Area | Current Rust behavior |
|---|---|
| Roles and skills | Preserves the 9 role and 33 skill IDs; coordinator role is fixed; privileged catalog edits |
| Worker qualifications | Provider-owned skill/role IDs; active Worker/Coordinator modes |
| Calendars | Approved redesign: one per worker, default weekly capacity, ISO-week overrides and minute reservations |
| Assignment | Approved redesign: all required skills, active mode, capacity and relevant score; random exact ties; no role filter |
| Projects/coordinators | Client creates a project; provider selects its coordinator |
| Planning | Added negotiated planning fee, duration, acceptance and separate delivery payment |
| Proposals/task storage | Draft/PendingApproval/Approved/Cancelled; provider creates storage per milestone; task tracking preserved |
| Funds | Approved full execution escrow and per-requirement/coordinator payouts replace legacy advance-only and first-worker payouts |
| Reputation | Approved per-milestone, committed-minute weighted scores replace standalone legacy project ratings |
| Cancellation/dispute | Party authorization, recorded reason, frozen funds; arbitration/refunds deliberately excluded |
| Storage | Mock memory/SQLite features, SQLite default, missing-only mock catalog seeding |
| Auth/signing/events | Classic cookie login, real sr25519 custody signatures, durable operations, event ingestion and SSE |

## Milestone task storage

This area is implemented in the Rust provider and covered by the expanded E2E:

- Creating a proposal atomically creates one task storage for every milestone.
- The storage has its own generated ID and records its owning milestone ID. Provider
  validation rejects duplicate IDs or a storage attached to the wrong milestone.
- Updating a draft preserves the milestone, storage ID and tasks when its stable
  milestone key remains. A new key creates a new milestone/storage; removing a key
  removes that storage with the draft aggregate. Deleting the draft removes all its
  storages in the same provider transaction.
- Only the coordinator creates or fully edits tasks. Assigned workers may update
  status and logged minutes. Clients are read-only. Cancelled projects reject writes.
- Tracking assignees and logged time do not alter contractual assignments,
  reservations, budgets or payouts. There is no task-delete command.
- The four-milestone E2E proves four distinct storage IDs survive a draft edit and
  execution approval, and that tasks do not leak between storages.

One semantic difference remains undecided: legacy `main` requires at least one task
in every milestone before proposal submission. SPEC-0003 does not require that, and
the Rust provider currently permits an empty storage. Decide this explicitly before
calling task-storage behavior fully equivalent. The legacy uses a separate in-memory
storage map and header-based mock caller; Rust keeps the storage inside the signed,
atomic project aggregate, which is an intentional architecture improvement.

Not preserved or not equivalent:

- Rich client/developer fields such as GitHub username, biography, background,
  proficiency, location and languages are absent from the current wire model.
- Skill-to-role catalog associations and free-text skill creation during profile
  updates are absent. Qualifications reference existing catalog IDs.
- Every milestone becomes `InProgress` on execution approval. The legacy activates
  milestones sequentially. Full-proposal atomic reservation is specified; whether
  sequential activation is still wanted needs a separate explicit decision.
- There is no explicit project `Completed` state or `mark_completed` command.
  Completed milestones and a cancellation flag are exposed instead.
- Cross-milestone assignment-key continuity is absent; approved matching selects by
  skills, mode, score and available capacity rather than preferring previous workers.
- Virto WebAuthn, community membership/governance, Kreivo RPC and Bramp
  deposit/withdrawal compatibility mocks are not ported to the current API.
  [SPEC-0004](../../specs/0004-virto-compatibility/plan.md) now inventories their
  legacy operations and proposes implementation tasks. It is a draft, not delivery.
- Generic standalone payment/refund/dispute-resolution endpoints are absent.
  Marketplace escrow is implemented; fund resolution is explicitly out of POC scope.

The profile, catalog-association, project-completion and sequencing differences must
not be described as verified feature parity. Decide whether to restore them before
claiming a complete legacy port. This review does not change approved business rules.

## Executable happy path

From the Rust repository root, with Rust 1.96.1 and Python 3:

```sh
cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked
python3 scripts/poc-e2e.py
```

The script defaults to both storage backends. Select one with `--storage sqlite` or
`--storage memory`. It opens local ports, starts real wallet/provider/adapter binaries,
generates disposable secrets and databases, and removes them after stopping its
  processes. It does not require Docker, a blockchain or real funds.

Observed 2026-09-15: both `PASS sqlite` and `PASS memory`. The first sandboxed attempt
could not create a socket; the unchanged test passed with local socket access.

Expanded verification on the same date: `cargo build -p adapter-api -p wallet -p
mock-provider --all-features --locked` and `python3 scripts/poc-e2e.py` passed.
Both `exercise` and `exercise_multi_milestone` passed on SQLite and memory (four
successful runs). See [the E2E handoff](../../progress/handoffs/E2E-legacy-scale.md).

Reverified on 2026-09-16 after the test-layout and mock-provider refactors:
46/46 workspace tests passed, both isolated mock feature matrices passed, and
the real-service E2E again reported all four SQLite/memory scenario passes.
Workspace format, check, Clippy, doc tests, build and WASM check passed. The
dependency-policy gate still fails for the previously documented CC0/Boost
allowlist and two Leptos maintenance advisories; `cargo audit` exits 0 with
those two maintenance warnings. See the [current review handoff](../../progress/handoffs/PORT-REVIEW-2026-09-16.md).

Covered flow: registration/login, catalog, worker registration, privileged coordinator
promotion and funding, planning quote/acceptance/payment, proposal and task storage,
execution approval and reservations, task progress, milestone completion, exact
payouts and weighted scores. It also checks CORS/CSRF/access denials, password changes
preserving wallet identity, lost successful reply recovery, exact retries, notification
ingestion, SSE replay, explicit read state and logout.

## Coverage limits

The Rust E2E retains the original one-client/coordinator/worker/outsider security
scenario and adds a separately initialized scenario with 10 workers, 2 coordinators
and 4 milestones with teams of 5/3/4/2 workers. Five workers have no available
capacity. The test checks all required skills despite differing roles, individual
task storages preserved through draft edits/approval, task progress, per-milestone payouts, exact replay, accumulated
minute-weighted scores, delegated scoring and weekly commitments.

The legacy scenario uses 10 workers, 2 coordinators and 4 milestones with 5/3/4/2
workers, checking unavailable candidates, repeated assignment continuity, preserved
storage identities during proposal edits, sequential activation and project completion.
The expanded Rust test covers the multi-team lifecycle under the approved redesign,
not legacy assignment-key continuity, sequential activation or explicit project
completion. Rust provider tests separately add concurrent approvals/rollback,
delegation, signature/replay, capacity and dispute checks.

`apps/leptos-web/tests/browser_smoke.py` runs Chromium against an HTTP fixture using
built static assets in `apps/leptos-web/dist`; it is separate from the real-service
E2E. `infra/verify.py` tests Nginx using a disposable upstream. Neither is a browser
happy path through every real service. The prior real Compose smoke only established
healthy services, mounted UI, public schema and private route denial.

To build browser fixture assets using the same direct tooling as the container:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo build -p leptos-web --bin leptos-web --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir apps/leptos-web/dist --out-name leptos_web \
  target/wasm32-unknown-unknown/release/leptos-web.wasm
install -m 644 apps/leptos-web/index.direct.html apps/leptos-web/dist/index.html
install -m 644 apps/leptos-web/style.css apps/leptos-web/dist/style.css
install -m 644 infra/nginx/config.js apps/leptos-web/dist/config.js
python3 apps/leptos-web/tests/browser_smoke.py
```

The browser smoke needs Chromium available as `chromium` or `chromium-browser`.
These fixture assets are not required for the backend E2E or Compose deployment.

## Remaining work

| Priority | Area | Status / next decision |
|---|---|---|
| P0 | Dispute resolution | [SPEC-0005](../../specs/0005-dispute-opening/status.md) implements versioned submission/rejection, public URL/SHA-256 evidence, a formal Open case, project-wide freeze, one response and signed E2E on both mock backends. Resolution/unlock, DAO authority, chat and timeout remain excluded and require a later specification |
| P0 | Verification closure | POC-07 and TASK-005 remain open; diagnostic secret-marker evidence, the key-rotation scope and dependency-policy gate still need closure |
| P1 | Virto auxiliary compatibility | Membership/governance remark and Bramp are planned in SPEC-0004; permissions and settlement rules await approval |
| P1 | Worker/client profiles | GitHub username, biography, background, proficiency, location and languages are absent |
| P1 | Catalog relations | Skill-to-role associations and legacy free-text skill creation are absent |
| P2 | Lifecycle parity choices | Sequential milestone activation, assignment-key continuity and explicit project completion need product decisions; current behavior follows SPEC-0003 |
| P2 | Optional compatibility | Virto WebAuthn/password-derived login, generic Kreivo RPC, generic payments and old contract wrapper aliases need a named consumer before implementation |
| Quality | Code structure | Provider domain and long mock integration scenarios were split; unit tests now live in `tests.rs`. The adapter and frontend were not split merely for line count; their flow and tests remain focused |

The unused service, worker and library scaffolds were removed. Calendar, tasks and
task storage remain domain logic inside the atomic mock provider; SSE remains in the
adapter. The approved backend runtime remains adapter, wallet and mock provider.
