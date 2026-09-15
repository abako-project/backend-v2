# REST adapter

The adapter owns credentials, sessions, durable transport operations, and notification
read state. Business state remains in the signed provider. Frontends use the same
REST routes and authenticated SSE; no frontend assets or arbitrary signing API are
served here. See `crates/generated-contracts/API.md` and `contracts/openapi.json`.

## Runtime

Required environment variables:

- `ADAPTER_DATABASE_URL`: SQLite URL, such as `sqlite:///data/adapter.sqlite?mode=rwc`.
- `CUSTODY_URL` and `MOCK_PROVIDER_URL`: private HTTP service origins.
- `INTERNAL_SERVICE_TOKEN_FILE`: service token file, at least 32 bytes.
- `BOOTSTRAP_ADMIN_PASSWORD_FILE`: initial administrator password file.
- `ALLOWED_ORIGINS`: comma-separated exact browser origins, without trailing slashes.

Optional: `BIND_ADDR` defaults to `0.0.0.0:8080`, `BOOTSTRAP_ADMIN_USERNAME`
to `admin`, `OPENAPI_PATH` to `contracts/openapi.json`, and `COOKIE_SECURE` to
`true`. Set the latter to `false` only for local HTTP. `/health` and `/ready`
are internal deployment probes, not public gateway routes.

SQLite migrations run before admission. Argon2 work runs on blocking workers with
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

The POC reads a bounded whole provider snapshot; paginate this boundary when data
outgrows that ceiling. This is not a production identity or custody assessment.

## Verification

```sh
cargo test -p adapter-api --offline
cargo clippy -p adapter-api --all-targets --offline -- -D warnings
```

The local suite uses real HTTP listeners and SQLite with deterministic custody and
provider doubles. The integrator's end-to-end check uses the real signing services.
