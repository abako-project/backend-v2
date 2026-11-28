# REST adapter

The adapter owns identity, sessions, descriptive content, catalog names, task
text/progress, durable operations and notifications in PostgreSQL. Enforceable business facts remain
in the signed provider. Frontends use the same
REST routes and authenticated SSE; no frontend assets or arbitrary signing API are
served here. See `crates/protocol/API.md` and `contracts/openapi.json`.

## Public platform profile

`GET /api/platform` returns DAO branding, currency display metadata, project
types and terminology before login. It uses the same Origin/CORS rules as
other public routes. The bundled profile is software/Digesta; its `decimals: 0`
matches current integer mock amounts. No balance, price or asset is converted.

Set optional `PLATFORM_CONFIG_PATH` to a readable JSON file with the response
shape to serve another profile. Use the bundled
[example](src/platform/default.json) as a starting point. Mount the file
read-only in the adapter container and set the variable to its container path.
No rebuild is needed. Restart to apply changes; all replicas must use the same
file. Invalid explicit configuration prevents startup instead of serving the
default. The file is limited to 64 KiB, each string to 4,000 UTF-8 bytes and
project types to 1–100 unique nonblank strings. All fields are public; do not
include secrets. Currency decimals is an unsigned integer from 0 through 255.

Brief `projectType` accepts each exact configured label, including spaces,
and the legacy values Other, SmartContract, Frontend, MVP, Audit and MobileApp.
Unknown values return 400. Saved types remain readable after configuration
changes; edits must use a currently allowed value. No migration is needed,
but rolling back to an older enum-only adapter after saving custom values
requires compatibility handling. This config represents one profile per
deployment; it does not create multi-tenant isolation or generic external
identity storage. Render terminology as text and icons as library identifiers.

## Runtime

Set these environment variables; only `ADAPTER_DATABASE_URL` is optional:

- `ADAPTER_DATABASE_URL`: PostgreSQL URL for a non-Compose deployment. If unset,
  the adapter uses `postgres://digesta:digesta-local-only@postgres:5432/digesta_adapter`,
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

### On a fork of Kreivo

Part of [moving the backend to Kreivo](https://github.com/abako-project/backend-v2/issues/46), in
progress. With `KREIVO_URL` set, the API needs neither the wallet nor the
provider service: it runs the contracts on a fork of Kreivo in its own process
([chain-provider](../../crates/chain-provider/README.md)), and `CUSTODY_URL`,
`MOCK_PROVIDER_URL` and `INTERNAL_SERVICE_TOKEN_FILE` are not read.

- `KREIVO_URL`: `fork:<path to a snapshot of Kreivo>`.
- `KREIVO_DATA_DIR`: where the fork and its state are saved. Keep it with the
  database: they describe the same marketplace.
- `KREIVO_CONTRACTS_DIR`: the directory of `dao.contract` and `project.contract`.
- `DERIVATION_SECRET_FILE`: a secret of at least 32 bytes that every password's
  device key is derived with. Losing it locks every account.
- `KREIVO_COMMUNITY` (optional): the ID of a community to set up on the fork
  at the first start, with a usage subscription, and to admit every account
  to. Its pool then pays the accounts' fees, as for the members of a
  subscribed community on Kreivo. Without it accounts pay their own fees from
  the balance they are given. Storage deposits come from that balance either
  way. The community is set up at the first start only: a saved fork starts
  again only with the same setting.

In this mode a user is a pallet-pass account and their password is its device:
the API derives the device key from the password when a request carries it,
and keeps a session key per account, in memory. While a passkey is being
added it also keeps the password's device key, for five minutes at most. A
passkey is a second device, limited to contract calls and to opening
sessions; the chain verifies its answers. A session lasts 15 minutes, the
longest Kreivo allows today, and a restart ends every session. A login opens
a new session, unless the same device opened one with more than ten minutes
left, which it shares: its cookie then lasts that session's remainder. The
public routes and their answers are the same; the session cookie's lifetime
is the session's, not the 24 hours `contracts/openapi.json` describes.

- The administrator's pass account is the Dao's operator. Its first device
  comes from the bootstrap password, at the first start.
- A passkey works only on the page whose host is `WEBAUTHN_RP_ID`: the chain
  ties a passkey to the host of the page that created it. The API does not
  start unless `WEBAUTHN_RP_ID` is the host of `WEBAUTHN_ORIGIN`.
- The administrator's commands come from the administrator's own pass
  account, not from a separate system wallet.
- `ENABLE_MOCK_FUNDING` reaches the Dao at the fork's first start only.
- A command whose submission ends without saying whether it was included is
  sent again, and can then run twice or be recorded as refused though it
  took effect ([limits](../../crates/chain-provider/README.md#limits)).
- A login with a passkey uses the passkey registered last.
- A password that is lost cannot be reset: nothing but its device, or a
  passkey for the calls a passkey may make, speaks for the account.

A fresh PostgreSQL schema is initialized before admission; compatible state reopens,
and unmarked existing schemas are rejected without modifying their data. `/ready` checks PostgreSQL and internal
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
cargo test -p api --all-features --locked
cargo clippy -p api --all-targets --all-features --locked -- -D warnings
```

Adapter tests require a disposable PostgreSQL instance via
`TEST_ADAPTER_DATABASE_URL`; see `src/tests.rs`. The integrated
`scripts/poc-e2e.py` starts its own temporary PostgreSQL instance and uses real
signing services and the contract provider, or with `--kreivo-snapshot` the
API alone on a fork.

## Code map

`http.rs` assembles public routes. Identity lives in `auth`, content in the named
profile/project/proposal modules, and delivery in `operations` and `notifications`.
`tasks` groups content execution, confirmation of contract membership, dispute
holds, views and history. Its `store` module supplies one task-row codec and write
path inside the caller's transaction. It does not own a separate queue or database.
