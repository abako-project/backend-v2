# POC05: Independent Leptos frontend

Specs: APPROVED SPEC-0002 and SPEC-0003; ADR-0001; shared HTTP contracts.
Branch: `feat/poc-frontend`. Worktree: `historical frontend worktree`.
Write scope: `apps/leptos-web/**` and this handoff. Shared lockfile changes are left to the integrator.

## Delivered

CSR application consuming typed REST requests with credentialed cookies, CSRF and operation idempotency. Includes account management, worker qualifications and weekly calendar, coordinator administration, planning quotes, proposals, milestone completion/ratings, task tracking, cancellation and disputes. Cancellation/dispute controls are available to the client and assigned coordinator, not other workers. Provider authorization remains authoritative.

Operations distinguish acknowledgement, unknown outcome and successful execution. Interrupted submissions retain their operation ID and exact payload for retries. SSE deduplicates notifications without marking them read and cleans up on logout. Public worker calendars use commitment totals without private project IDs.

The static build uses `/config.js` and `window.KUNVENO_API_BASE` (default `/api`). No backend implementation or database dependency is imported. Browser state is session-local; an existing operation can be queried by its displayed reference after reload. The notification view is bounded to 500 received items; complete history remains accessible through the API.

## Verification observed 2026-09-10

- `cargo fmt -p leptos-web -- --check`: passed.
- `cargo test -p leptos-web --locked`: 3 tests passed (week/catalog validation, finality versus outcome, exact reputation display).
- `cargo clippy -p leptos-web --target wasm32-unknown-unknown --locked -- -D warnings`: exit 0. Existing root clippy MSRV mismatch warning remains for the integrator's root configuration update.
- From `apps/leptos-web`: `env -u NO_COLOR trunk build --release --locked`: passed, including WASM binding and static distribution.
- `python3 apps/leptos-web/tests/browser_smoke.py`: passed with local-socket sandbox escalation. Real Chromium mounted the built WASM and verified login cookie, CSRF, typed worker payload, idempotency key, unknown outcome followed by successful polling, SSE duplicate/read behavior, and logout cleanup.

Browser smoke uses an explicit HTTP fixture, not actual custody/provider processes. Full-stack domain and proxy verification belong to integration. No production-readiness claim.
