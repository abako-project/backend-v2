# Porting and E2E coverage

Reviewed 2026-09-15 against Rust implementation commit `3ab2b58` plus documentation
and native-command corrections. This is a port of the approved marketplace redesign,
not a complete compatibility port of the legacy backend or all mock endpoints.

## Source baseline

The original repository is `legacy backend repository` (main `45a4f79`
at review). The owner selected its development worktree `legacy task-storage worktree`,
branch `feat/68-provider-owned-proposals-task-storages`, commit `d408641`, for the port.
The Rust repository remains separate. Legacy `/v1` clients need adaptation to `/api`.

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
processes. It does not require Docker, RTK, a blockchain or real funds.

Observed 2026-09-15: both `PASS sqlite` and `PASS memory`. The first sandboxed attempt
could not create a socket; the unchanged test passed with local socket access.

Covered flow: registration/login, catalog, worker registration, privileged coordinator
promotion and funding, planning quote/acceptance/payment, proposal and task storage,
execution approval and reservations, task progress, milestone completion, exact
payouts and weighted scores. It also checks CORS/CSRF/access denials, password changes
preserving wallet identity, lost successful reply recovery, exact retries, notification
ingestion, SSE replay, explicit read state and logout.

## Coverage limits

The Rust E2E uses one client, one coordinator, one worker, one outsider and one
milestone with one requirement. It reaches settlement, but is not the full legacy
scenario in `packages/adapter-api/test/projects-happy-path.e2e-spec.ts`.

The legacy scenario uses 10 workers, 2 coordinators and 4 milestones with 5/3/4/2
workers, checking unavailable candidates, repeated assignment continuity, preserved
storage identities during proposal edits, sequential activation and project completion.
Rust provider tests add concurrent approvals/rollback, delegation, signature/replay,
capacity and dispute checks, but do not supply an equivalent multi-team lifecycle.

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

Remaining test work: a multi-milestone/multi-worker scenario matching the approved
semantics, complete diagnostic secret-marker checks, and independent POC-07/TASK-005
acceptance/security reports. The key-rotation scope and dependency policy also remain
open; this successful E2E does not close those gates.
