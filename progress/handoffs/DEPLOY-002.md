# DEPLOY-002: Backend moved to dev.abako.xyz

Date: 2026-10-21. Status: COMPLETE.

User instruction: deploy at `dev.abako.xyz/api`, without `web.*`.
[SPEC-0011](../../specs/0011-backend-dev-host/spec.md) supersedes SPEC-0010's
hostname choice and retains its isolation and data-preservation requirements.

Primary writer: root agent, integrator. Branch:
`agent/integrator/DEPLOY-002-dev-host`; worktree `/tmp/digesta-dev-host`;
base `9198f7e`. Allowed/changed paths: SPEC-0011 directory; `infra/preview/`
installer, provisioner, settings example and new `move-host.py`;
`docs/operations/cloud.md`; `docs/operations/digesta-backend-preview.md`;
and this handoff. No Rust, dependency, lockfile, schema or contract changes.

## Result and decisions

- API: <https://dev.abako.xyz/api/>
- Schema: <https://dev.abako.xyz/api/openapi.json>, 80 paths.
- The web-host `/api/openapi.json` now falls through to its existing frontend;
  it no longer returns the Digesta schema. No redirect or second backend route
  was added there. The exact web `/api/kreivo-rpc` route and its cookie-exclusion
  directive remain intact.
- Keep the same published `v0.2.0` image digests, Compose project, identities,
  credentials, secrets and stores. Only the Digesta adapter was recreated;
  the other eight container identities and all nine image IDs are unchanged.
- Set `PREVIEW_HOST=dev.abako.xyz`, `WEBAUTHN_RP_ID=dev.abako.xyz`, and
  `WEBAUTHN_ORIGIN=https://dev.abako.xyz`. Cookies remain Secure, HttpOnly,
  SameSite=Lax and scoped to `/api`.
- Exact allowed origins are `https://dev.abako.xyz` and
  `https://web.dev.abako.xyz`. The latter preserves the existing frontend's
  client access while the backend is served only at the corrected host.
  An untrusted origin remains rejected. No wildcard CORS or parent-domain
  cookie was added.

Existing `dev.abako.xyz` TLS was valid for at least the following week. No DNS,
certificate, firewall, global Nginx configuration or legacy container changes
were needed. The selected dev virtual host is stored as
`/etc/nginx/sites-available/abako-backend`; its filename is not its hostname.

The dev site had server-level legacy OPTIONS handling and CORS headers. The
new API excludes `/api` from that preflight shortcut so the adapter handles
its own OPTIONS requests. The API location retains the site's five existing
security response headers and uses the adapter's CORS headers. Other routes
retain the legacy behavior. Nginx header inheritance and graceful reload
behavior follow the [official headers reference](https://nginx.org/en/docs/http/ngx_http_headers_module.html#add_header)
and [reload guidance](https://nginx.org/en/docs/beginners_guide.html#control).

## Commands and observed evidence

SSH used the user-selected `digesta` alias, explicit `-F ~/.ssh/config`, no
agent forwarding and a temporary ControlMaster. The user confirmed readiness
for the successful YubiKey retry. The explicit config bypassed an unreadable
system SSH include; no local SSH permissions or system files were changed.

| Command/check | Result |
| --- | --- |
| Read-only route/TLS/container inspection | PASS: no existing dev `/api/` route; both certificates valid; all nine containers healthy |
| Public checks before cutover | Dev schema HTTP 404; web schema HTTP 200, Digesta title and 80 paths |
| Python AST parsing of all six deployment scripts | PASS |
| Focused site-builder checks | PASS: preserve root/HTTP redirect, reject conflicting/wrong/duplicate API sites, preserve quoted semicolons in HSTS, isolate legacy preflight/CORS |
| Isolated rollback checks using temporary files and stubbed commands | PASS: refuse later edits before any write; restore all three original files/modes; target only adapter and Nginx; no data/volume deletion |
| `python3 /opt/digesta/preview/move-host.py` | PASS: private backups and combined candidate `nginx -t`; no active change |
| `python3 /opt/digesta/preview/move-host.py --apply` | PASS: hostname cutover, healthy adapter, public identity/login, signed operation and SSE |
| Additional external credentialed preflight check | Found inherited legacy OPTIONS handling; corrected API exclusion and header inheritance, then retested |
| Staged and active `nginx -t` after CORS correction | PASS; graceful reload |
| Final API verifier, base `https://dev.abako.xyz`, origin `https://dev.abako.xyz` | PASS: stable account, secure cookie, CSRF/origin denial, fresh signed command, persisted receipt/worker and SSE replay |
| Same verifier against dev API with frontend origin `https://web.dev.abako.xyz` | PASS: same authenticated command/SSE checks |
| Independent external schema/CORS/private-route/legacy preflight checks | PASS: corrected schema, no web Digesta schema, exact credentialed headers for both allowed origins, untrusted-origin HTTP 403, private/encoded paths HTTP 404, legacy OPTIONS HTTP 204 |
| Container IDs/images/settings/ports | PASS: all nine healthy, all image IDs unchanged; only adapter recreated; exact RP/origin/cookie settings and loopback 4080 verified |
| Frontend root and legacy `/health` response hashes | PASS: unchanged from this task's pre-cutover baseline |
| Repository/server file hash comparison | PASS: all nine deployment files match |
| Documentation relative links and staged whitespace check | PASS |

The initial isolated header candidate failed Nginx validation because a quoted
HSTS semicolon was truncated; it was never activated. Complete directive lines
are now retained and covered by the focused checks. Public CORS checks also
wait for new workers after a graceful reload; a check of the old worker caused
one safe restoration before this readiness polling was added. Final tests pass.

Only deployment gates were applicable. This task reuses the already verified
release images and changes no Rust/dependencies; it does not claim new Cargo,
coverage or full backup/restore runs. The original coherent snapshot/restore
evidence remains in [DEPLOY-001](DEPLOY-001.md).

## Configuration backups and rollback

Final cutover backup:
`/srv/digesta/preview/backups/dev-host-20261021103306`.
It contains original `dev.conf`, `web.conf`, `deployment.env`, final candidates,
`record.json` with before/after hashes and a `verified` marker. Files are private
and remain on the server. A separate check-only candidate directory at
`/srv/digesta/preview/backups/dev-host-20261021103258` did not change live state.

| Site file | Before SHA256 | Final SHA256 |
| --- | --- | --- |
| `abako-backend` | `96b55dc4ee803901c2b1aff6fea5bde826e0e39779ed0910ca32b0ca8473f407` | `12e6f494c026d8bbf714a5e6357e4028343b51fe4781d515bd6f6f2b0dafc95b` |
| `web.dev.abako.xyz` | `79a547d2789ac04f4fa528bb0e1e8f38f2174f45851566564f0d8d12ffa9a401` | `9cf87e6c59d821b91e47346c2c516783e3ab5cb44adb44d22164eedaac5a2fb9` |

Guarded rollback command, run as root:

```bash
python3 /opt/digesta/preview/move-host.py \
  --rollback /srv/digesta/preview/backups/dev-host-20261021103306
```

It refuses changed backups or later edits, restores the previous host settings
and both sites, recreates only the adapter and validates/reloads Nginx. Clients
would then use the previous web-host API. It does not restore or delete data.
The live rollback was not deliberately exercised after final success; isolated
checks cover its refusal/restoration behavior, and failure handling safely
restored the prior Nginx site during the cutover checks.

Pre/post-cutover frontend response SHA256:
`372dcb4aab78ecb17b2ba37e8141b936ad43c339c82c98a5b16f0bfbc21601c8`.
Legacy health SHA256:
`63745aef95742025c6d7a1b4fc0e7107e6f3c3eb2e0cb290f33c46f176e6740d`.

## Covered scenarios, risks and blockers

Both acceptance scenarios pass. No backend-testing blocker remains.
Clients must update the API base URL and log in on the new host; old host-scoped
cookies are not transferred. Existing password accounts and shared fixtures
were retained without reseeding. Passkeys for the former RP may need enrollment
on the new RP. Actual browser passkey ceremonies and frontend deployment were
not exercised; password/session/command/SSE flows were verified with both client
origins. Real assets, production custody, off-host recovery and legacy data
migration remain outside this mock-backed preview task.
