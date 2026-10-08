# BE-B: Project submission brief

Status: implemented and verified; pending owner review.
Specification: approved BE-B amendment in SPEC-0006.
Branch: `feat/project-submission-brief`.
Worktree: `/home/clara/Documents/kunveno/worktrees/backend-project-submission`.
Base: backend master `4bde1f5417898bf94a6fa355c45e22e15cb3a0a1`.

## Behavior

GET/PUT `/api/projects/{projectId}/brief` persist descriptive project submission
data in adapter PostgreSQL: summary, legacy project type, optional HTTP(S) link,
ordered objectives and constraints, indicative USD budget range and delivery
preference/date. Title and description remain in the provider project. No financial
conversion, task storage, matching, planning or evidence behavior changes.

Only the current project client writes, with cookie and CSRF. Project participants
read using the existing visibility rule; outsiders and absent projects get 404. An
authorized existing project without a brief gets 200 null. Provider unavailable
returns 503; authorization is not inferred from a stored brief.

First save expects revision 0 and returns 201/revision 1. Updates return 200 and
increment once. Identical immediate retries return the saved result; other stale
writes return 409 `brief_revision_conflict`. A unique composite key and transactional
row locks protect concurrent replicas. Provider instance isolation prevents old
briefs attaching to IDs reused after reset. The migration is additive/repeatable.

After a confirmed project creation, retain its project ID and save the brief. A
failed brief save must retry that write only, never repeat project creation.

## Files changed

| File | Change |
|---|---|
| `services/adapter-api/src/project_briefs/mod.rs` | GET/PUT handlers; current provider visibility and client-only writes. |
| `services/adapter-api/src/project_briefs/models.rs` | Typed brief, enums, revision and text/URL/date validation. |
| `services/adapter-api/src/project_briefs/store.rs` | Parameterized PostgreSQL reads/writes, revision conflict and exact retry behavior. |
| `services/adapter-api/src/project_briefs/tests.rs` | Validation boundaries and OpenAPI example roundtrips. |
| `services/adapter-api/src/project_briefs/http_tests.rs` | Real HTTP/PostgreSQL permissions, errors, persistence and cross-replica concurrency. |
| `services/adapter-api/migrations/0006_project_briefs.sql` | Adapter-owned JSONB table keyed by provider instance/project. |
| `services/adapter-api/src/main.rs` | Module registration. |
| `services/adapter-api/src/http.rs` | Brief route within marketplace routes, using the existing cookie/CSRF boundary. |
| `services/adapter-api/src/state.rs` | Transactional startup migration registration. |
| `services/adapter-api/src/tests.rs` | Reuse the existing HTTP/dependency test harness. |
| `contracts/openapi.json` | Two operations, request/response schemas, enums, limits, errors and examples. |
| `crates/generated-contracts/tests/openapi.rs` | Update the checked public operation count from 72 to 74. |
| `scripts/poc-e2e.py` | Verify brief persistence/retries/permissions and unchanged business state inside the existing signed lifecycle. |
| `docs/project/happy-path.md` | Explain two-stage creation/save, recovery and separate descriptive revisions. |
| `specs/0006-backend-parity/spec.md` | Approved adapter-owned brief contract. |
| `specs/0006-backend-parity/acceptance.feature` | Brief privacy/persistence/concurrency acceptance scenarios. |
| `specs/0006-backend-parity/status.md` | Amendment and automated verification status. |
| `progress/handoffs/BE-B.md` | This review and validation record. |

## Validation

Executed from the isolated worktree, with `CARGO_BUILD_JOBS=2` and the existing
compilation cache. Rust database tests used a disposable PostgreSQL 17 container
and separate per-test schemas; acceptance launched temporary PostgreSQL, wallet,
provider and adapter processes. No existing users, balances or databases changed.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass. |
| `cargo check --workspace --all-targets --all-features --locked` | Pass. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Pass. |
| `cargo nextest run --workspace --all-features --test-threads 2` | 76/76 pass. |
| `cargo test --workspace --doc` | 2/2 pass. |
| `cargo build --workspace --all-targets --all-features --locked` | Pass. |
| `cargo deny check` | Existing failures: CC0-1.0/BSL-1.0 allowlist and unmaintained paste/proc-macro-error2. |
| `cargo audit` | Pass with two unmaintained warnings for those crates. |
| `python3 scripts/poc-e2e.py --storage both --binaries <compiled-target>/debug` | Eight real-service scenarios pass. |
| `python3 -m py_compile scripts/poc-e2e.py` | Pass. |
| `git diff --check` | Pass. |

Dependencies, Cargo.lock and dependency-policy files are unchanged. Codebase
Memory indexed the isolated worktree; new Rust paths have matching metadata.
Migration, acceptance script and documentation were inspected directly because
they are excluded from that index.

Covered: client/coordinator/assigned-worker/outsider and anonymous access; missing
project; absent brief; CSRF/Origin rejection; HTTP 400/422/413 and normalized errors;
Unicode text limits and U+0000 rejection; ordered fields, enum and nested unknown
field validation; HTTP(S) links; leap-year dates; exact create/update retries;
stale conflicts; missing initial revision; concurrent first saves and different
updates from two App instances; adapter restart/repeat migrations; provider reset;
provider unavailable; unchanged provider project state, balances and submission
count. The real lifecycle repeats brief checks before planning and after worker
assignment in both provider storage modes, then completes existing settlement
and dispute acceptance scenarios.

## Review and reproducible checks

1. Inspect the brief examples and operations in `contracts/openapi.json`.
2. Build the worktree: `cargo build --workspace --all-targets --all-features --locked`.
3. With local PostgreSQL tools installed, run
   `python3 scripts/poc-e2e.py --storage both --binaries <compiled-target>/debug`.
   It creates and removes its own temporary stack. It does not use localhost:8088
   or the existing application database.
4. For the Rust suite, point `TEST_ADAPTER_DATABASE_URL` at a disposable PostgreSQL
   database and run `cargo nextest run --workspace --all-features --test-threads 2`.
5. In a deployment containing this branch: create a project, confirm its operation
   receipt, then PUT the documented brief with expectedRevision 0. Repeat unchanged
   (200, revision still 1), update using revision 1 (revision 2), and attempt a
   different stale edit (409). GET as coordinator/assigned worker (200), outsider
   (404); PUT as coordinator/worker (403). Verify the business project and balances
   remain unchanged.

## Limits and next work

No commit, push, merge or deployment was performed. Main backend/frontend worktrees
and their existing uncommitted changes were preserved. Existing local services
continue running the previously deployed version, which does not have these routes.

B1–B3 frontend recovery and browser comparison against Figma/legacy have not begun.
Chrome/Playwright visual validation belongs to that frontend task; this backend
change was exercised over real HTTP with real adapter/custody/provider processes.
Milestone hash removal is a separate approved direction requiring its own scoped
contract change; current delivery and dispute evidence requirements remain intact.
The legacy indicative budget is descriptive USD, with no exchange rate or automatic
funding effect. Acceptance of this backend change does not approve scope-review,
chat, ratings, dispute-resolution or other later phases.

## Local integration update — 2026-10-08

At the owner's request, BE-B was applied to the backend base worktree without
a commit or Git merge. Existing local changes were preserved. The local
`abako-auth-review` adapter now runs `kunveno-adapter-brief-review:local`, built
from that base worktree with two compilation jobs. PostgreSQL migration applied;
OpenAPI includes both brief routes and anonymous access returns the expected 401.
The wallet, provider, databases, balances and volumes were retained. CORS now also
allows the isolated Phase B frontend at `http://localhost:5175`. No remote
deployment, commit or push was performed. The earlier no-deployment statement
above describes the original isolated validation only. Chrome verified a disposable non-admin client registering (201), saving its
profile (200), creating one project (202 then Finalized/Success), saving its brief
(201/revision 1), retrying the identical write (200/revision still 1), and reading
it back (200 with unchanged ordered data). Frontend wizard integration remains
pending; this was a direct API smoke test from the browser, not a wizard E2E.

## Consolidation and development fixtures — 2026-10-08

At the owner's request, master is now the only backend checkout. The contact/catalog
and frontend-submodule branches were already ancestors of master; the duplicate
BE-B worktree matched master byte-for-byte. All three redundant worktrees were
removed after saving patches/untracked files under /tmp/backend-consolidation-backup.
Branches and existing unrelated main-checkout changes were preserved.

The local abako-auth-review PostgreSQL, custody and mock stores were reset with
backups, preserving runtime secrets and unrelated databases. The main-built adapter
image remains deployed at localhost:8088. scripts/seed-dev-users.py reuses the
existing API test client to create 10 workers, 3 eligible Coordinator-mode workers
and 3 clients; profiles, registration, promotion and simulated deposits all use
public APIs. Each fixture has 1000 KVN. Repeating the script verifies/reuses those
actors without adding duplicate workers or topping up already funded balances.
Both application README files list the disposable development credentials.

Verification in master: 76/76 workspace tests with temporary PostgreSQL and two
test threads; fmt, workspace check and strict clippy passed. The first test attempt
failed because TEST_ADAPTER_DATABASE_URL was not configured; the isolated rerun
passed. Existing dependency-policy findings in the original validation remain.
Frontend browser validation created one project and saved/retried its brief after
a lost HTTP response and reload: one project creation, revision 1, no duplicate.
Hash-removal work and the detailed B2/B3 visual review remain separate pending work.
