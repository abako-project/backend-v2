# DEPLOY-001: Backend preview deployed

Date: 2026-10-15. Status: COMPLETE for the approved mock-backed backend preview.

Task/spec: [SPEC-0010](../../specs/0010-backend-preview-deployment/spec.md),
APPROVED by the user's parallel deployment, backend-first testing and explicit
selection of `https://web.dev.abako.xyz/api/`. The user authorized SSH with the
`digesta` alias and physical YubiKey touch. No new production deployment,
real-asset custody, DNS change or legacy migration is included.

Primary writer: root agent, integrator. Branch:
`agent/integrator/DEPLOY-001-backend-preview`. Worktree:
`/tmp/digesta-backend-deploy`. Base: `28c9afa`.

## Changed files and decisions

- `specs/0010-backend-preview-deployment/`: approved scope, design, data
  ownership, threat model, tasks and acceptance scenarios.
- `infra/preview/`: digest-pinned Compose, public settings example, Nginx
  snippet and five provisioning/activation/fixture/API/restore scripts.
- `docs/operations/cloud.md`: current approved deployment state.
- `docs/operations/digesta-backend-preview.md`: test accounts, client guidance,
  server operations, backup and rollback.
- This handoff records observed results. Application code, dependencies,
  migrations, contracts, workspace files and lockfiles are unchanged.

The unavailable DNS record for `digesta.dev.abako.xyz` was replaced by the
user-selected existing HTTPS site. Its current certificate was reused. The
existing frontend root and exact Kreivo RPC location remain present. Nginx
strips cookies only from that external RPC location to prevent disclosure of
Digesta sessions. The new API keeps its `/api` prefix and supports SSE without
proxy buffering. Access logging uses the path without query values.

The new Compose project owns a fresh PostgreSQL volume, separate wallet/mock
SQLite stores and matching private secrets. It has no legacy database access.
Applications run as the non-login `digesta` UID 999/GID 987 with read-only root
filesystems, capability drops, PID limits and bounded logs. Only the adapter
publishes `127.0.0.1:4080`; the other services share an internal Docker network.
Server configuration is `/opt/digesta/preview`; state is `/srv/digesta/preview`.

## Released artifacts

The deployment uses the successful
[v0.2.0 release run](https://github.com/abako-project/backend-v2/actions/runs/38712832177),
tagged at `bdbbaa8e770f83b9a636f6fec75b130d8f713e54` on master. Anonymous registry
manifest checks and actual server pulls verified the application digests:

| Image | SHA256 digest |
| --- | --- |
| `ghcr.io/abako-project/digesta-adapter-api` | `c121368e5d11061be93e466ec2483046b4270cf61b8b456640f3c664ae4e7dd2` |
| `ghcr.io/abako-project/digesta-wallet` | `88a41ae8c369b1e96e26f2daf3c8aa769bf1795efa493c17bdc8a1dd6f0aa5ef` |
| `ghcr.io/abako-project/digesta-mock-provider` | `68b0c1a7199ef159031508b90f33bae5af27c4c3afe15d61210e16bdaeeb5bc6` |
| `postgres` (17.11-alpine3.24) | `b0f9560a2de083e2cc7382e75f808c7381a32852a7ec49117deedb300e552b24` |

## Commands and observed results

Deployment files were uploaded to `/opt/digesta/preview`, alongside unchanged
`scripts/poc-e2e.py` and `scripts/seed-dev-users.py` client helpers. Actual secret
values were generated and kept on the server; none entered Git or command output.
SSH used a temporary ControlMaster with agent forwarding disabled.

| Command/check | Observed result |
| --- | --- |
| Python AST parsing for all five deployment scripts | PASS |
| Local deployment-file hashes compared with all eight server copies | PASS: reviewed files match deployed files |
| Documentation link checks and staged `git diff --cached --check` | PASS |
| Focused Nginx block parser checks: nesting, comments, quotes, escapes, incomplete block rejection | PASS |
| `docker compose ... config --format json` with disposable placeholder files | PASS: four pinned images, isolated ports/networks, secure settings, health dependencies and bounded logs |
| `python3 /opt/digesta/preview/provision.py --host web.dev.abako.xyz` | PASS: dedicated identity and private secrets; four containers healthy |
| Fixture seeder using the exact command below | PASS: 10 workers, 3 eligible coordinators, 3 clients; at least 1000 mock KVN each |
| `python3 /opt/digesta/preview/verify-api.py ... --internal-http` on loopback 4080 | PASS: authentication, CSRF/origin denial, signed commands, state and SSE |
| `python3 /opt/digesta/preview/verify-restore.py` | PASS: coherent offline snapshot, healthy active restart, isolated restore and retained stopped clone |
| Updated API verifier on retained restored clone | PASS: a fresh signed command succeeds, proving restored key/state usability |
| `python3 /opt/digesta/preview/install-nginx.py` | PASS: staged full Nginx config test, actual `nginx -t`, graceful reload and private original-site backup |
| Public API verifier with HTTPS base/origin `https://web.dev.abako.xyz` | PASS: secure cookie, stable ordinary identity, denial checks, fresh signed command and SSE replay through proxy |
| Independent OpenAPI fetch from outside the server | HTTP 200, Digesta title, 80 paths |
| Credentialed CORS preflight | HTTP 204, exact allowed origin, credentials enabled |
| Public `/api/internal`, `/api/internal/signing-jobs`, encoded internal path, `/api/health`, `/api/ready` | HTTP 404 |
| Private readiness and all new container health checks | PASS, all four healthy |
| Secret directory/file modes and bounded application log scan for current secret markers | PASS: directory 0700, files 0600, no matching values in checked logs |
| Existing frontend root and legacy backend `/health` | HTTP 200, response hashes unchanged |
| Existing five `abako-dev` containers | Original IDs unchanged, all healthy |
| Existing exact Kreivo route read-only `system_chain` probe | HTTP 403; direct request to original `kreivo.kippu.rocks` upstream also HTTP 403 |

The fixture seeding command was:

```bash
python3 /opt/digesta/preview/seed-fixtures.py \
  --base-url http://127.0.0.1:4080 \
  --origin https://web.dev.abako.xyz \
  --admin-password-file /srv/digesta/preview/secrets/bootstrap-admin-password \
  --internal-http
```

`--internal-http` adjusts only the verification client's cookie jar to send the
secure cookie over trusted loopback. The server's secure-cookie policy remains
enabled; the later public HTTPS verifier uses the actual secure cookie unchanged.

Only relevant deployment gates were rerun: no Rust or dependency changes were
made. The released artifacts already passed workspace formatting, Clippy with
warnings denied, Nextest with PostgreSQL, doc tests and the container builds in
the linked release run. This handoff does not claim new Cargo audit/deny or
coverage runs for this deployment task.

## Recovery evidence

Snapshot: `/srv/digesta/preview/backups/20261015102542`, containing `state.tar`,
`postgres.tar` and `verification.json`. All preview services were stopped for
the snapshot; the active stack restarted healthy before the isolated restore.

Restore project: `digesta-restore-20261015102542`. Separate environment:
`/srv/digesta/preview/restore-check/20261015102542/deployment.env`. It verified
stable login/account identity, completed original receipt, persisted worker,
SSE history and a fresh signed provider command. It is stopped, with separate
containers, bind stores and volume retained. No volumes were deleted.

Original site backup:
`/srv/digesta/preview/backups/nginx/web.dev.abako.xyz-1792060243.conf`.
Original SHA256: `4faa42137905283d1c2284b4a2bb67e4cc5eb419fab365f31e954df9fda79917`.
Deployed SHA256: `79a547d2789ac04f4fa528bb0e1e8f38f2174f45851566564f0d8d12ffa9a401`.
The [operations guide](../../docs/operations/digesta-backend-preview.md) includes
guarded rollback commands that preserve all data and the legacy services.

Unchanged frontend response SHA256:
`8313b55c34af33fb8b3f685de5cb969e73fb5f36f4ea008aaad41cc193929662`.
Unchanged legacy health response SHA256:
`63745aef95742025c6d7a1b4fc0e7107e6f3c3eb2e0cb290f33c46f176e6740d`.

## Covered scenarios, limits and blockers

All three SPEC-0010 acceptance scenarios were exercised: public HTTPS without
frontend delivery; credentialed signed operations with denial checks; and
isolated services with verified restored state. The frontend-team guide gives
the current API/schema URLs and shared fixture accounts.

There is no blocker to backend testing. The new frontend and its local browser
proxy have not been tested in this task. Local password login needs a trusted
HTTPS development proxy; passkeys require the configured public hostname.
No wildcard CORS, insecure cookie policy or public internal service access was
introduced. The preserved external Kreivo upstream refusal remains unresolved
and is outside the mock-backed backend dependency path.

The snapshot is private and local to the host. Scheduled/off-host backups,
production monitoring, production key management and real-asset migration
remain separate work. The server's existing reboot-required marker was left
untouched; no reboot or legacy resource cleanup was performed.
