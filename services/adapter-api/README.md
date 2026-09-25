# REST adapter

The adapter owns credentials, sessions, descriptive profiles, durable transport
operations, and notification read state in PostgreSQL. Business state remains
in the signed provider. Frontends use the same
REST routes and authenticated SSE; no frontend assets or arbitrary signing API are
served here. See `crates/generated-contracts/API.md` and `contracts/openapi.json`.

## Runtime

Set these environment variables; only `ADAPTER_DATABASE_URL` is optional:

- `ADAPTER_DATABASE_URL`: PostgreSQL URL for a non-Compose deployment. If unset,
  the adapter uses `postgres://kunveno:kunveno-local-only@postgres:5432/kunveno_adapter`,
  valid for the default local Compose service. Set an explicit secure URL elsewhere.
- `CUSTODY_URL` and `MOCK_PROVIDER_URL`: private HTTP service origins.
- `INTERNAL_SERVICE_TOKEN_FILE`: service token file, at least 32 bytes.
- `BOOTSTRAP_ADMIN_PASSWORD_FILE`: initial administrator password file.
- `ALLOWED_ORIGINS`: comma-separated exact browser origins, without trailing slashes.

Optional: `WEBAUTHN_RP_ID` defaults to `localhost` and `WEBAUTHN_ORIGIN` to
`http://localhost:8088`; configure both for the browser's exact RP/origin.
Non-local origins must use HTTPS. `BIND_ADDR` defaults to `0.0.0.0:8080`, `BOOTSTRAP_ADMIN_USERNAME`
to `admin`, `OPENAPI_PATH` to `contracts/openapi.json`, `COOKIE_SECURE` to
`true`, and `ENABLE_MOCK_FUNDING` to `false`. Set `COOKIE_SECURE=false` only
for local HTTP. The funding flag exposes fixture-only `/api/admin/fund`; a
normal mock deployment uses simulated Bramp deposit requests. `/health` and `/ready`
are internal deployment probes, not public gateway routes.

Fresh PostgreSQL migrations run before admission; existing SQLite development
data is not imported or deleted. `/ready` checks PostgreSQL and internal
dependencies. Argon2 work runs on blocking workers with
bounded admission. Sessions use opaque hashed tokens, HttpOnly SameSite=Lax cookies,
and session-bound CSRF tokens. Password changes preserve the custodial account and
revoke other sessions. Administrator commands select the separate system wallet
internally; callers cannot supply an origin or wallet.

## Delivery and recovery

Operations are durably ordered per wallet and conditionally leased. Their exact
SCALE payload and signed envelope are stored before submission. Ambiguous delivery
remains `OutcomeUnknown`, blocks later operations for that wallet, and reconciles
the existing receipt. It never silently creates a replacement business operation
or retargets a signed payload after provider reset. A finalized receipt still has
an independent execution outcome.

Provider events and ingestion cursors commit together. SSE resumes using adapter
notification IDs and never marks notifications read. Read state changes only via
the explicit authorized mutation. Existing notification history survives provider
instance resets. The public worker directory strips reservation project IDs.

The adapter also exposes canonical `/api/task-storages/{storageId}` and task
routes while retaining project-nested write aliases. Descriptive profile edits
are adapter-local, not signed business commands; mock Bramp mutations are signed
business commands. Workers can submit skill requests, and only the operator can
approve or reject them; approval does not qualify the requester automatically.
Passkey registration, listing, removal and username-first login use the same
principal, custodial wallet and session contract as password login. An email
address is not required or verified for this login method.

The current mock-backed integration reads a bounded whole provider snapshot;
paginate this boundary when data outgrows that ceiling. This is not a production
identity or custody assessment.

## Verification

```sh
cargo test -p adapter-api --all-features --locked
cargo clippy -p adapter-api --all-targets --all-features --locked -- -D warnings
```

Adapter tests require a disposable PostgreSQL instance via
`TEST_ADAPTER_DATABASE_URL`; see `src/tests.rs`. The integrated
`scripts/poc-e2e.py` starts its own temporary PostgreSQL instance and uses real
signing services with each mock storage backend.
