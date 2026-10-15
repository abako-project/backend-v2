# SPEC-0010: Backend preview deployment

Status: APPROVED

Task: DEPLOY-001

Owner: Digesta product owner

Approval: On 2026-10-15 the user accepted the parallel server proposal, selected
a `digesta.*` subdomain, and directed that the backend be deployed first for the
frontend team's testing. The frontend follows later. When DNS could not be added, the user explicitly
selected `https://web.dev.abako.xyz/api/` and approved keeping its existing
frontend and Kreivo RPC route running.

## Scope

Deploy the released mock-backed backend on the `digesta` SSH host,
`128.140.7.148`, as a separate `digesta-preview` Compose project. Publish the API
at `https://web.dev.abako.xyz/api`. Keep the existing applications and data
running. This is a development preview; real assets and production custody are
outside this task.

The root agent is the sole integrator/writer on
`agent/integrator/DEPLOY-001-backend-preview`, worktree
`/tmp/digesta-backend-deploy`, based on `28c9afa`. Allowed repository paths:
this specification directory, `infra/preview/`, `docs/operations/cloud.md`,
`docs/operations/digesta-backend-preview.md`, and
`progress/handoffs/DEPLOY-001.md`. Existing application files are read-only.

Allowed server changes: a dedicated non-login Digesta service account; new
configuration under `/opt/digesta/preview`; isolated state/secrets/backups under
`/srv/digesta/preview`; this Compose project's containers, networks and volume;
the new API route in the existing `web.dev.abako.xyz` Nginx HTTPS virtual host,
with validated graceful reloads and a rollback copy of the original site. Reuse
its existing certificate. Preserve the frontend root and the exact Kreivo RPC
route; prevent application session cookies from reaching that external RPC.
Other virtual hosts, containers and databases must not be replaced or reset.
Allow a temporary `digesta-restore-*` project and isolated stores for the
required restore drill. No DNS or certificate changes are needed.

## Requirements

- DEPLOY-001: Use the published adapter, wallet and mock `v0.2.0` digests verified
  by the successful release run. Resolve PostgreSQL 17.11-alpine3.24 to a digest.
- DEPLOY-002: Run application processes as the dedicated non-root UID/GID,
  with private secrets, isolated persistent stores, restart policies, health
  checks and bounded container logs. Publish only adapter loopback port 4080.
- DEPLOY-003: Host Nginx serves HTTPS, forwards `/api/` with its prefix intact,
  supports authenticated SSE and blocks public internal/health/readiness routes.
  No frontend build or deployment is required. A plain root response may point
  testers to `/api/openapi.json` without exposing operational details.
- DEPLOY-004: Preserve secure SameSite=Lax session cookies, CSRF checks, exact
  allowed origins and service-token authentication. Use the configured preview
  HTTPS origin and hostname for WebAuthn. Do not relax cookie policy for local
  cross-site frontend testing; document API-client and same-origin proxy options.
- DEPLOY-005: Verify public OpenAPI, login/session cookie, authenticated
  mutations, CSRF/origin rejection, operation completion, SSE replay and private
  route denial. Check that the old sites and containers remain available.
- DEPLOY-006: Keep fresh preview state and matched secrets separate from old
  records. Prove a coherent backup/restore of this new environment before
  handoff. Retain a rollback command that stops only the new stack and disables
  only its public API route. No deletion or legacy migration is authorized.
- DEPLOY-007: Provide frontend-team API URL, schema URL, authentication/CSRF/
  idempotency guidance and the exact current verification/deployment state.

## Dependencies and failure behavior

Public HTTPS uses the existing hostname and certificate. Keep the backend
bound to loopback until proxy configuration is validated, and do not claim a
working public URL until verified. Touch-backed SSH authentication requires
the user to be present.

If health, authentication, DNS, certificate, proxy validation or restore fails,
record the exact error and retain existing services. Never overwrite an existing
secret set, restore over active state or remove a persistent volume.
