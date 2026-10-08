# BE-A1 / BE-A2 — Developer signup compatibility

Date: 2026-10-08. Status: reviewed and approved for commit, merge into master and push on 2026-10-08.

## Ownership and authority

Branch: `feat/signup-contact-catalog`.
Worktree: `/home/clara/Documents/kunveno/worktrees/backend-contact-catalog`.
Base: `926808c`. Main backend checkout and its user changes were preserved.

Explicit user approval: “Apruebo los cambios, aplicalos en el backend y continua”. Approved parity-spec amendment: `specs/0006-backend-parity/spec.md`. Existing REST ADR and OpenAPI remain authoritative. Scope is BE-A1/BE-A2 only; B/C are separate executors.

## Changes

- `services/adapter-api/src/http.rs`: public GET `/api/catalog` only. Origin, limits and all mutation security remain.
- `services/adapter-api/src/profiles/models.rs`: optional nullable worker contact email, bounded to 254 UTF-8 bytes, no whitespace/control, one @ with nonempty parts. Public projection unchanged.
- `services/adapter-api/src/profiles/store.rs`: parameterized private read/upsert of contact email.
- `services/adapter-api/migrations/0005_worker_contact_email.sql`: nullable column with IF NOT EXISTS; existing rows keep null.
- `services/adapter-api/src/state.rs`: run migration in the existing explicit startup transaction.
- `services/adapter-api/src/profiles/tests.rs` and `src/tests.rs`: validation, backward-compatible omission/null, private persistence, public exclusion, second principal isolation, public catalog and protected routes/Origin checks.
- `contracts/openapi.json`: only public catalog security/description and private WorkerProfile property.
- `crates/generated-contracts/tests/openapi.rs`: update the existing public-read security expectation.
- `specs/0006-backend-parity/spec.md`: record the approved username/contact/catalog distinction.

PUT omission/null clears contact email, as proposed and approved. No verification or mail delivery; syntax is deliberately bounded rather than a full RFC mailbox parser. Client profile, images, provider, wallet, matching, blockchain and lockfile are unchanged.

## Verification

Ran with CARGO_BUILD_JOBS=2 and the existing target cache. Adapter/database tests used a new disposable PostgreSQL 17 container; it was removed after tests.

| Gate | Result |
| --- | --- |
| cargo fmt --all -- --check | Passed |
| cargo check --workspace --all-targets --all-features --locked | Passed |
| cargo nextest run --workspace --all-features --no-fail-fast | 71 passed |
| cargo test --workspace --doc | 2 passed |
| cargo build --workspace --all-targets --all-features --locked | Passed |
| cargo clippy --workspace --all-targets --all-features --locked -- -D warnings | Existing duration_suboptimal_units warnings in bloque-rs accounts.rs:188/215 and config.rs:92; failed |
| cargo clippy -p adapter-api --all-targets --all-features --locked -- -D warnings -A clippy::duration_suboptimal_units | Passed; explicit baseline exception, not a green global gate |
| cargo deny check | Existing license allowlist failures CC0-1.0/BSL-1.0 and unmaintained paste/proc-macro-error2; failed |
| cargo audit | Exit 0 with two unmaintained warnings |

Frontend tests/build and real Playwright signup also passed. Anonymous catalog 200; account 201; private profile 200; image 204/download 200; worker 202 and finalized successful receipt. Contact email absent from public profile. CSRF and idempotency present where required. Real writes with an injected single 503 proved retries and reload recovery without duplicate accounts. Cookie remains HttpOnly/SameSite=Lax/Path=/api. 20 hours persist as 1,200 minutes; zero remains zero.

## Local application and integration

Temporary adapter-only Dockerfile and Compose image override are under `/tmp/abako-auth-tools-1000/`; no infrastructure source edits. Running Compose project `abako-auth-review` uses `kunveno-adapter-signup-review:local` for adapter only. Existing services, bind configuration and database were preserved; gateway reloaded. Adapter restarted successfully; contact email, image and session persisted. API: `http://localhost:8088/api`; frontend review: `http://localhost:5173/register/developer`.

Review docs and screenshots: phase A frontend worktree, `frontend/docs/reviews/auth-A5-A8.md`.

Integration must sequence the shared middleware/OpenAPI/startup files with B/C. If another backend branch already owns migration number 0005, agree on the sequence and update its startup include before merge. No automatic integration has occurred.

## Integration approval and final recheck

The user explicitly requested committing and merging this backend branch into master and pushing it on 2026-10-08. Frontend Phase A is complete and published as `eb9da23`; its integration into `feat/backend-api-compatibility` has merge commit `16a4cd1`.

Before the backend commit, the branch was advanced without conflicts to `origin/master` at `ca1df7a`, preserving the approved changes. This includes the upstream CI release workflow and Duration/Clippy corrections; no release tag or production deployment was requested or created.

Final gates on that updated base: fmt, workspace/all-targets/all-features check, **Clippy with -D warnings without an exception**, build, **71 Nextest tests** and **2 doctests** pass. Database tests used disposable PostgreSQL 17 on localhost; the container was stopped and removed. Logs: `/tmp/phase-a-merge-gates/`. `cargo audit` exits 0 with the existing two unmaintained warnings. `cargo deny` still fails for existing CC0-1.0/BSL-1.0 license policy and paste/proc-macro-error2 maintenance advisories; the lockfile and dependency policy were not changed.

The main backend checkout's modified `.gitignore` and three untracked review documents must remain local and outside these commits. Shared middleware/OpenAPI/migration changes remain limited to BE-A1/BE-A2. No B/C backend changes are present in this branch. Existing frontend Phase 14 work is also preserved and excluded from the frontend merge.

Remaining limitations: email is unverified contact data with bounded syntax, not an authentication identifier. No real-hardware passkey test was repeated during integration. Deployment of these commits requires the adapter startup migration; the local review adapter already uses it. Wallet authentication remains unavailable by agreement.
