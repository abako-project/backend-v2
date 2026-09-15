# POC-04 adapter handoff

Task: implement the independent REST adapter under approved SPEC-0001 through
SPEC-0003, ADR-0001, and the frozen HTTP contracts. Branch `feat/poc-adapter`,
worktree `historical adapter worktree`. Scope: `services/adapter-api/**` and this file.

Implemented SQLite credentials/sessions, typed authorized routes, per-wallet
durable signing/submission queues, lease fencing, immutable payload recovery,
private operation lookup, durable event ingestion, SSE replay, explicit read
state, administrator operations, and public worker-calendar privacy filtering.
Only the project client or assigned coordinator may cancel or open disputes.

Checks: `rtk proxy cargo test -p adapter-api --offline` and
`rtk proxy cargo clippy -p adapter-api --all-targets --offline -- -D warnings`.
Focused checks cover HTTP authentication/CORS/CSRF/password changes, idempotency
ownership, ordered leases and stale holders, lost submission replies, exhausted
submission retries and provider resets, cancellation/dispute permissions,
notification privacy, transactional replay, and explicit read state.

The branch's shared `clippy.toml` retains an old MSRV metadata warning; the
integrator owns its already prepared correction. No root manifest, contract,
lockfile, or unrelated work is included in this task's commit. Full workspace,
real cryptographic end-to-end, and Compose verification belong to integration.

No task blocker. POC limitation: bounded whole-snapshot provider reads, not paged
queries. No production readiness claim.
