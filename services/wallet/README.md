# Custody service

Implements SPEC-0001 for local mock use. It has no provider client and never submits
transactions. Only the custody database contains encrypted seeds; public responses
contain wallet metadata, job status and signatures.

## Runtime

Required environment variables:

| Variable | Value |
|---|---|
| `CUSTODY_DATABASE_URL` | Separate SQLite database, e.g. `sqlite:///data/custody.sqlite?mode=rwc` |
| `CUSTODY_MASTER_KEY_FILE` | Private file containing a 32-byte hexadecimal encryption key |
| `CUSTODY_ROOT_SEED_FILE` | Separate private file containing the system-origin seed |
| `INTERNAL_SERVICE_TOKEN_FILE` | Private file containing a high-entropy bearer credential |
| `BIND_ADDR` | Default `127.0.0.1:8081`; use `0.0.0.0:8081` inside Compose |

Secret files must be regular files with no group/other permissions. Master key and
root seed are never read from the database. A changed root identity, wrong master
key, damaged ciphertext or mismatched stored account prevents startup/readiness.
The host/process owner remains trusted. These keys must never control real assets.

Create development secrets outside the repository:

```sh
cargo run -p wallet -- init-dev-secrets /tmp/kunveno-local-secrets
```

The new directory is mode `0700`; all five files are created with `0600`, fresh OS
randomness and `create_new`. Existing targets are refused, not overwritten. A
partial filesystem failure may leave newly created files; inspect the directory
before generating a replacement. Nothing is printed to stdout.

Local operator lifecycle changes require the same protected startup configuration:

```sh
cargo run -p wallet -- set-lifecycle <wallet-id> Suspended
cargo run -p wallet -- set-lifecycle <wallet-id> Retired
```

Only `Active -> Suspended` or `Active -> Retired` is accepted. Repeating the same
transition is harmless. There is no public lifecycle or key-export endpoint.
Ciphertext and audit history are retained. Key rotation/reactivation are not
implemented without an approved recovery design.

## Internal HTTP

The four custody routes in `crates/generated-contracts/API.md` require bearer
authentication. Job creation returns `202`, including exact idempotent retries.
Different bytes or metadata with the same operation ID return `409`. Invalid or
expired new payloads return `400`; queue admission failure returns `429`.

`GET /health` and `GET /ready` return `204` without credentials, strictly on the
internal listener. `GET /internal/metrics` requires the bearer credential and
returns secret-free queue, outcome, attempt, oldest-age and lease-recovery counters.
Responses disable caching. The reverse proxy must expose none of these routes.

Four observed workers lease durable jobs. A partial unique index permits one claim
per wallet; creation sequence preserves queue order. Thirty-second expired leases
are recoverable, and completion checks the lease token, deadline and current wallet
lifecycle in one transaction. Crypto runs on bounded blocking workers outside the
SQLite transaction. The adapter separately owns signing/submission serialization.

The only supported payload is canonical, validated mock SCALE version 1, with a
matching origin, operation ID, expiration and SHA-256 digest of the exact bytes.
New call lifetime is at most five minutes. Pending jobs are limited to 32 per wallet
and 10,000 per instance. A failed local storage operation retains durable state;
workers back off and stop the service after five consecutive failures. Recovery
reclaims leases rather than inventing a new operation or signature payload.

## Verification

```sh
cargo test -p wallet --offline
cargo clippy -p wallet --all-targets --offline -- -D warnings
```

Tests use disposable SQLite and real sr25519 verification. The HTTP test opens an
ephemeral loopback port and therefore needs local network permission in a sandbox.
