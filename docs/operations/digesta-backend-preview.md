# Digesta backend preview

Deployed on 2026-10-15 for frontend integration, using the published `v0.2.0`
backend images and a mock provider. The existing frontend remains in place;
the updated frontend will follow separately.

- API base: <https://web.dev.abako.xyz/api/>
- OpenAPI: <https://web.dev.abako.xyz/api/openapi.json> (80 paths)
- Approved scope: [SPEC-0010](../../specs/0010-backend-preview-deployment/spec.md)
- Verification evidence: [DEPLOY-001 handoff](../../progress/handoffs/DEPLOY-001.md)

## Test accounts

These are shared development fixtures, with at least 1000 mock KVN each at
seeding time. Transactions can change their balances. All assets are simulated.

| Usernames | Password pattern | Example |
| --- | --- | --- |
| `worker1` through `worker10` | `Worker N 1234!` | `worker1`: `Worker 1 1234!` |
| `coordinator1` through `coordinator3` | `Coordinator N 1234!` | `coordinator1`: `Coordinator 1 1234!` |
| `client1` through `client3` | `Client N 1234!` | `client1`: `Client 1 1234!` |

Coordinators have worker profiles and coordinator eligibility. The bootstrap
administrator credential stays in a private server file. Use ordinary fixture
accounts for frontend tests; personal passwords and real assets do not belong
in this preview.

## Authentication and commands

Send `POST /api/auth/login` with JSON `username` and `password`. The response is
a session view containing `accountId` and `csrfToken`; the HTTP response sets
`digesta_session`. Include cookies on subsequent requests and send the session
view's token as `X-CSRF-Token` on authenticated mutations. `GET /api/auth/session`
returns the current session view.

The cookie is `Secure`, `HttpOnly`, `SameSite=Lax`, scoped to `/api`, and has a
24-hour lifetime. The only allowed origin is `https://web.dev.abako.xyz`.
Missing/invalid CSRF and other origins are rejected. API clients such as curl
or Postman should use a cookie jar and this exact `Origin` header.

Provider commands also require `Idempotency-Key`: `0x` followed by 32 hexadecimal
digits. Keep the same key and payload when retrying the same command; create a
new key for a new command. A `202` response gives an `operationId`. Poll
`GET /api/operations/{operationId}` until a terminal state and inspect the
receipt outcome; accepting the command alone does not mean it succeeded.
The [OpenAPI contract](../../contracts/openapi.json) defines which endpoints
are commands and their payloads.

`GET /api/events` streams authenticated SSE. Include credentials and retain the
last event ID for reconnection. An SSE client can send `Last-Event-ID` to replay
later persisted events. Native `EventSource` manages its reconnect cursor;
use `{withCredentials: true}` when appropriate.

## Running the new frontend locally

The deployed cookie and origin rules prevent direct credentialed requests from
a frontend on `http://localhost:5173`. Use a local HTTPS development server that
proxies its own `/api` requests to the deployed API. This keeps browser requests
on the local frontend's origin. The proxy must verify the upstream TLS certificate
and set the upstream `Origin` to the deployed origin. Keep the `/api` prefix.

For a Vite frontend, merge the following into its existing server configuration:

```js
server: {
  host: 'localhost',
  // https must use a certificate trusted by the local browser.
  https: { key: localTlsKey, cert: localTlsCertificate },
  proxy: {
    '/api': {
      target: 'https://web.dev.abako.xyz',
      changeOrigin: true,
      headers: { Origin: 'https://web.dev.abako.xyz' },
    },
  },
}
```

`localTlsKey` and `localTlsCertificate` are certificate contents supplied by the
frontend's existing development setup. Call relative `/api/...` URLs with
credentials enabled. This is integration guidance; the new frontend and this
browser setup have not been deployed or tested in this task. Test password login
through the local proxy. Passkey ceremonies require the configured WebAuthn
hostname `web.dev.abako.xyz`, so a localhost proxy cannot test those ceremonies.

Cookie behavior follows [MDN's Set-Cookie reference](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie).
Vite's HTTPS and proxy options are described in its
[server configuration reference](https://vite.dev/config/server-options).

## Server layout and checks

Connect using `ssh digesta`; authentication requires the YubiKey touch.
Operate the preview as root on this host:

- Configuration and scripts: `/opt/digesta/preview`
- Private state and secrets: `/srv/digesta/preview`
- Non-login application user: `digesta`, UID 999, GID 987
- Compose project: `digesta-preview`
- PostgreSQL volume: `digesta-preview_adapter-postgres-data`
- Adapter proxy target: `127.0.0.1:4080`; wallet, mock and database have no
  published host ports.

All four images are digest-pinned in [compose.yaml](../../infra/preview/compose.yaml).
The three application containers run as the dedicated user with read-only root
filesystems and bounded logs. PostgreSQL uses its supported entrypoint and
dedicated volume. Actual deployment settings are in root-only `deployment.env`;
the checked-in example's numeric IDs are placeholders.

```bash
docker compose --env-file /opt/digesta/preview/deployment.env \
  -f /opt/digesta/preview/compose.yaml ps
curl --fail --silent http://127.0.0.1:4080/ready
python3 /opt/digesta/preview/verify-api.py \
  --base-url https://web.dev.abako.xyz \
  --origin https://web.dev.abako.xyz \
  --identity-file /srv/digesta/preview/secrets/smoke-identity.json
```

The verifier checks authentication, cookie flags, CSRF/origin denial, a fresh
signed mock command, persisted state and SSE replay. It updates the private
smoke account and prints no credentials. Public `/api/internal/...`,
`/api/health` and `/api/ready` are denied; operational probes use loopback.
The existing site's `/health` remains its legacy route.

The exact `/api/kreivo-rpc` location remains routed to its original upstream.
It now excludes application cookies so the external RPC cannot receive Digesta
session cookies, as described by
[Nginx's proxy header rules](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_set_header).
A read-only `system_chain` probe returned HTTP 403 through the preserved route
and directly from its original upstream, `kreivo.kippu.rocks`. The upstream
refusal remains unresolved.
The deployed mock-backed Digesta API does not depend on this RPC route.

## Backup, restore and rollback

The verified coherent snapshot is
`/srv/digesta/preview/backups/20261015102542`. It contains private `state.tar`
and `postgres.tar`, plus `verification.json`. Adapter, custody and provider
were stopped together for the snapshot and restarted healthy. Restoring into
`digesta-restore-20261015102542` preserved login identity, receipts and SSE
history; a new signed command also succeeded. The clone and its separate stores
remain stopped under `/srv/digesta/preview/restore-check/20261015102542`.
No legacy stores were copied or modified.

To create and verify another snapshot, run:

```bash
python3 /opt/digesta/preview/verify-restore.py
```

This briefly stops only the preview. Backups include secrets and require private
handling. The drill proves recovery of this preview snapshot; scheduled/off-host
backups, continuous recovery and production custody are not configured.

The original Nginx site is saved at
`/srv/digesta/preview/backups/nginx/web.dev.abako.xyz-1792060243.conf`.
For rollback, first check that the active site still matches the deployed hash
below. A changed hash needs review so later edits are not overwritten.
Then restore only this site, validate/reload Nginx, and stop only this preview:

```bash
set -e
sha256sum /etc/nginx/sites-available/web.dev.abako.xyz
# Expected: 79a547d2789ac04f4fa528bb0e1e8f38f2174f45851566564f0d8d12ffa9a401
test "$(sha256sum /etc/nginx/sites-available/web.dev.abako.xyz | cut -d ' ' -f 1)" \
  = 79a547d2789ac04f4fa528bb0e1e8f38f2174f45851566564f0d8d12ffa9a401
cp -p /srv/digesta/preview/backups/nginx/web.dev.abako.xyz-1792060243.conf \
  /etc/nginx/sites-available/web.dev.abako.xyz
nginx -t
systemctl reload nginx
docker compose --env-file /opt/digesta/preview/deployment.env \
  -f /opt/digesta/preview/compose.yaml stop
```

If validation fails, retain the running configuration and correct the candidate
before reloading. Rollback leaves data, secrets and volumes intact. The original
frontend, exact RPC location and other virtual hosts remain present.
