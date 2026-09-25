# Porting and E2E coverage

Reviewed 2026-09-24 against the backend integration branch
and legacy `main` commit `3e2b929`. Implementation claims below describe
the current integration, not a release on `master`.
This is a port of the approved marketplace redesign,
not a complete compatibility port of the legacy backend or all mock endpoints.

## Source baseline

Legacy `main` now includes `feat/68-provider-owned-proposals-task-storages` through
merge commit `3e2b929`; the feature implementation is commit `d408641`. The Rust
repository remains separate. Legacy `/v1` clients need adaptation to `/api`.

## Functional comparison

| Area | Current Rust behavior |
|---|---|
| Roles and skills | Preserves the 9 role and 33 skill IDs; coordinator role is fixed; privileged catalog edits. Skill-to-role associations are exposed; workers request additions and only the operator decides. Approval does not qualify the requester. |
| Worker qualifications | Provider-owned skill/role IDs; active Worker/Coordinator modes |
| Calendars | Approved redesign: one per worker, default weekly capacity, ISO-week overrides and minute reservations |
| Assignment | Approved redesign: all required skills, active mode and capacity; previous-milestone workers preferred when eligible, then relevant score and random exact ties; no role filter or `assignmentKey` |
| Projects/coordinators | Client creates a project; provider selects its coordinator |
| Planning | Added negotiated planning fee, duration, acceptance and separate delivery payment |
| Proposals/task storage | Draft/PendingApproval/Approved/Cancelled; provider creates storage per milestone; task tracking preserved; submission requires a task in every storage; canonical direct task-storage routes coexist with nested aliases |
| Milestone sequence | Execution approval reserves/assigns every milestone but activates only the first; accepting one activates the next; final acceptance marks project `completed` |
| Funds | Approved full execution escrow and per-requirement/coordinator payouts replace legacy advance-only and first-worker payouts |
| Reputation | Approved per-milestone, committed-minute weighted scores replace standalone legacy project ratings |
| Cancellation/dispute | Party authorization, current-submission rejection, public Open case, one response and project-wide freeze; resolution/refunds excluded |
| Storage | Adapter PostgreSQL; custody SQLite; mock memory/SQLite features with SQLite default; missing-only mock catalog seeding. No development-data migration. |
| Descriptive profiles | Adapter PostgreSQL stores separate editable client and worker sections; public projection excludes email, department, session and custody data. Auxiliary E2E passes in both mock modes. |
| Bramp | Mock-only deposit request with one operator credit; pending withdrawal hold and cancellation. Auxiliary E2E passes in both mock modes. No bank connection, currency conversion or generic payment API. |
| Auth/signing/events | Classic cookie login, real sr25519 custody signatures, durable operations, event ingestion and SSE. WebAuthn registration and username-first login use the same principal and wallet; a virtual-authenticator HTTP test covers the full login. No verified email is required. |

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
- A draft may have empty storages; submitting it requires at least one task in
  each storage. An unsuccessful submission leaves the draft unchanged.
- Canonical `/api/task-storages/{storageId}` and `/tasks/{taskId}` reads, plus
  direct POST/PUT/PATCH task writes, resolve the owning project in the adapter.
  Existing project-nested write paths remain aliases; there is one provider store.
- The four-milestone E2E fixture checks that four distinct storage IDs survive
  a draft edit and execution approval and that tasks do not leak between
  storages. The expanded assertions passed in both mock modes against adapter PostgreSQL.

Legacy uses a separate in-memory storage map and header-based mock caller;
Rust keeps storage inside the signed, atomic project aggregate. The task
submission precondition is now specified in SPEC-0003 and implemented.

An Open milestone dispute now freezes writes to all of the project's task
storages, not only the disputed milestone. Public case reads and the single
counterparty response remain available. No dispute resolution or unlock exists.

Open or deliberately changed compatibility points:

- Profile sections are stored in adapter PostgreSQL, not provider state.
  Their read/write/public-projection checks pass in the auxiliary E2E.
- Skill-to-role associations and worker new-skill requests are implemented
  under [SPEC-0006](../../specs/0006-backend-parity/spec.md). Qualifications
  still change only through the worker qualification command.
- Sequential activation and project completion are implemented. Team continuity
  means preferring an eligible previous worker, not preserving legacy
  `assignmentKey` identity. This is a deliberate matching rule change.
- WebAuthn passkey registration and username-first login are implemented under
  [SPEC-0004](../../specs/0004-virto-compatibility/spec.md). Password login
  remains available and both methods create a session for the same custodial
  wallet. Email verification is not part of this login policy.
- Bramp is a typed mock of deposit credit and withdrawal holds. It is not
  legacy `/v1` wire compatibility, a bank connection or a real Kreivo call.
- Membership, governance, DAO voting, generic payments/refunds and dispute
  resolution are excluded by approved scope. Project escrow alone pays workers
  and coordinators; an Open dispute freezes pending funds without unlock.

## Executable happy path

From the Rust repository root, with Rust 1.96.1, Python 3 and PostgreSQL
server commands `initdb`, `pg_ctl` and `psql`:

```sh
cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked
python3 scripts/poc-e2e.py
```

The script defaults to both mock storage backends. Select one with
`--storage sqlite` or `--storage memory`. It starts a disposable PostgreSQL
instance, real wallet/provider/adapter binaries and fresh secrets, then
removes their temporary data. It does not require Docker, a blockchain or
real funds. It enables fixture-only `/api/admin/fund`; normal Compose does not.

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

Reverified on 2026-09-23 after SPEC-0005: 49 workspace tests and workspace
format/check/Clippy/doc-test/build passed. The signed single-milestone,
four-milestone 5/3/4/2 and dispute scenarios passed on both SQLite and memory.
The dispute scenario ends with an Open public case, one response, frozen project
and unchanged balances. See [DSP-07](../../progress/handoffs/DSP-07.md).
`cargo-deny` was intentionally omitted at the owner's request; no passing
dependency-policy claim is made.

The 2026-09-24 fixture adds task-submission, canonical storage reads,
sequential activation and project-completion assertions. These passed against
adapter PostgreSQL with mock memory and SQLite. The auxiliary scenario also
passed in both modes, covering profiles, Bramp, catalog requests, passkey
boundary failures and bounded secret-marker scanning of service logs.

Covered flow: registration/login, catalog, worker registration, privileged coordinator
promotion and funding, planning quote/acceptance/payment, proposal and task storage,
execution approval and reservations, task progress, milestone completion, exact
payouts and weighted scores. It also checks CORS/CSRF/access denials, password changes
preserving wallet identity, lost successful reply recovery, exact retries, notification
ingestion, SSE replay, explicit read state and logout.
The new dispute scenario also covers versioned submission, rejection, public
read, authorization, freeze and one immutable counterparty response.

## Coverage limits

The Rust E2E retains the original one-client/coordinator/worker/outsider security
scenario, adds a separately initialized scenario with 10 workers, 2 coordinators
and 4 milestones with teams of 5/3/4/2 workers. Five workers have no available
capacity. The test checks all required skills despite differing roles, individual
task storages preserved through draft edits/approval, task progress, per-milestone payouts, exact replay, accumulated
minute-weighted scores, delegated scoring and weekly commitments.
It also runs a third, isolated dispute scenario. The fixture uses public
evidence URLs and hashes; the service does not fetch those URLs.

The legacy scenario uses 10 workers, 2 coordinators and 4 milestones with 5/3/4/2
workers, checking unavailable candidates, repeated assignment continuity, preserved
storage identities during proposal edits, sequential activation and project completion.
The expanded Rust fixture asserts sequential activation and explicit project
completion under the approved redesign, but not legacy assignment-key continuity.
Rust provider tests separately cover
concurrent approvals/rollback, delegation, signature/replay, capacity and disputes.

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
| Verified mock backend | Verification | Historical tasks POC-07 and TASK-005 have independent handoffs for the mock-backed backend. The E2E scans REST, SSE, custody metrics and bounded service logs for known secret markers; this is not a production secrecy proof. HSM-backed rotation remains mandatory before real value. Cargo-deny and Leptos gates were outside that backend closeout by owner decision. |
| Implemented in backend branch | Passkey access | Login starts with username and ends with the same session and wallet as password login; virtual-authenticator HTTP coverage is in the adapter tests. Email verification is not required. |
| P1 | Browser contract | Keep direct task routes and temporary nested aliases until external frontend clients migrate. OpenAPI describes implemented routes, including passkey login/options. |
| Out of scope | Governance and generic Virto wrappers | No membership/governance, DAO votes, generic payments or arbitrary Kreivo JSON-RPC. Existing typed project, calendar and escrow operations replace those legacy wrappers where applicable. |
| Future spec | Dispute resolution | [SPEC-0005](../../specs/0005-dispute-opening/status.md) ends at an Open case and frozen project. DAO authority, resolution/unlock, escrow disposition, chat and timeout need separate product decisions; they are not defects in the approved opening scope |
| Future platform | Real chain and production custody | Replace the signed mock-provider integration with reviewed chain encoding, submission, finality, HSM-backed key management and recovery before real assets |
| Quality | Browser-to-backend E2E | The signed E2E exercises real backend processes; browser smoke uses fixtures. No single browser test drives the full real-service lifecycle |
| Quality | Code structure | Provider domain and long mock integration scenarios were split; unit tests now live in `tests.rs`. The adapter and frontend were not split merely for line count; their flow and tests remain focused |

The unused service, worker and library scaffolds were removed. Calendar, tasks and
task storage remain domain logic inside the atomic mock provider; SSE remains in the
adapter. The approved backend runtime remains adapter, wallet and mock provider.
For the call-by-call successful flow, see [the happy-path guide](happy-path.md).
