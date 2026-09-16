# POC-01B: Public REST contract

## Scope and authority

- Task: complete the browser-facing OpenAPI contract for independent frontends.
- Specifications: approved SPEC-0002 and SPEC-0003; accepted ADR-0001.
- Branch: `feat/poc-contracts`.
- Worktree: `historical contracts worktree`.
- Writer: contracts agent; root owns manifests, lockfile, and integration.
- Previous commits: `b481ae4` frozen contracts; `9bd3234` privacy-safe worker views.

## Changed files

- `contracts/openapi.json`: OpenAPI 3.1.1 with 39 paths, 42 operations, and 75 schemas.
- `crates/generated-contracts/API.md`: exact admin routes and browser integration.
- `crates/generated-contracts/tests/openapi.rs`: reference, security, route-parameter,
  DTO-example, revision, privacy, and SSE checks.
- This handoff. No dependency or lockfile changes belong to this task.

## Decisions and interface

The public API uses REST under `/api`, without GraphQL or `/v1`. Internal provider
and custody endpoints are not exposed. GET `/api/openapi.json` reads the document
from `OPENAPI_PATH`, default `contracts/openapi.json`, in the adapter implementation.

The adapter agent confirmed `kunveno_session`, HttpOnly, SameSite=Lax, Path=/api,
Max-Age=86400, with configurable Secure. Registration returns 201; login/session
200; password/logout 204. Register/login/schema are public; other routes require
the cookie. Authenticated mutations additionally require session-bound CSRF.
Usernames are 3–64 ASCII allowed characters; passwords are 12–1024 UTF-8 bytes.

All provider mutations return 202 OperationRef. Idempotency keys bind principal
and typed action. An unknown transport outcome is reconciled, never interpreted
as rejection. Acceptance uses the revision the client reviewed. Money schemas
reject amounts outside u64 and use decimal strings, not floating-point numbers.

GET workers exposes WorkerSummaryView and per-week totals, never reservation
project IDs. SSE emits notification events with adapter notification IDs and
NotificationView JSON. Last-Event-ID overrides after. Notification page size is
100; receipt or replay never marks a notification read.

## Verification

- `cargo test -p generated-contracts --locked`: 7 unit tests and 3 new
  OpenAPI integration tests passed; doc tests passed (none defined).
- `cargo clippy -p generated-contracts --all-targets --locked -- -D warnings`:
  passed after correcting a documentation-markdown lint in the new test.
- `cargo fmt -p generated-contracts -- --check`: passed.
- `git diff --check`: passed before handoff.
- Node's native RegExp and BigInt checked the Money regex against zero, u64 bounds,
  malformed decimal forms, and 2,000 deterministic values up to 10^21: passed.

The existing root `clippy.toml` MSRV 1.85 differs from Cargo's 1.96.1; this emits an
existing warning but is outside this task's write scope. No dedicated OpenAPI
validator is installed. Verification here covers local references, security/path
structure and actual Rust serialization, not a claim of independent full-spec
validator certification. Cross-field invariants such as valid ISO week 53 and
score weights summing to 100 are documented and enforced by Rust validation.

## Sources and remaining integration

The serialization and documentation-writing skills kept wire constraints and
examples aligned with the approved specification. Context7 retrieved primary
OpenAPI documentation from the official OpenAPI specification repository,
version 3.1.1. No dependency was added.

The integrator must run the full workspace and HTTP smoke tests after applying
service implementations. In particular, serve this file in the adapter/container,
verify CORS/CSRF from two origins, exercise async status and SSE replay, and check
that implementation routes remain aligned with this document.
