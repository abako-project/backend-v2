# SPEC-0011: Correct the backend preview hostname

Status: APPROVED

Task: DEPLOY-002

Approval: On 2026-10-21 the user instructed deployment at
`dev.abako.xyz/api`, without `web.*`. This supersedes SPEC-0010's hostname
selection. Its isolation, private-service, data-preservation and mock-preview
requirements remain in force.

## Scope and ownership

Retain the existing web frontend as an exact allowed client origin.
Move the existing preview API to `https://dev.abako.xyz/api/` using the same
server, image digests, Compose project, secrets and stores. Update its exact
origin and WebAuthn hostname together. Withdraw only the added Digesta API
route from `web.dev.abako.xyz`; preserve its frontend and existing exact RPC
location, including the cookie-exclusion protection. Preserve unrelated routes
and legacy containers. Reuse the existing certificate; no DNS change is needed.

Allowed server changes: deployment scripts/settings under `/opt/digesta/preview`,
private configuration backups under `/srv/digesta/preview/backups`, the two
selected Nginx sites and a recreation of only the adapter container. No data,
volume, secret, legacy service, DNS or certificate deletion/replacement.

## Requirements

- HOST-001: Inspect current routing, certificate and container state before any
  server write. Do not overwrite conflicting API routes without recording them.
- HOST-002: Validate candidate Nginx configuration and preserve private copies
  of both sites and the deployment settings. Restore these on cutover failure.
- HOST-003: Publish the existing `/api` contract on the corrected HTTPS host,
  retaining its prefix, private-route denial and unbuffered SSE.
- HOST-004: Allow exact API origin `https://dev.abako.xyz` and retain frontend
  origin `https://web.dev.abako.xyz`. Configure WebAuthn RP ID
  `dev.abako.xyz`, its HTTPS WebAuthn origin and secure cookies. Keep CSRF,
  service authentication and privilege rules. Existing users can log in with
  their passwords on the new host; host-scoped browser cookies are not moved.
- HOST-005: Verify HTTPS schema, stable account identity, login, secure cookie,
  CSRF/origin denial, a fresh signed operation and SSE replay. Verify legacy
  health, frontend response and unaffected container identities.
- HOST-006: Update the testing guide and examples, record exact evidence and
  a guarded rollback procedure. Do not report deployment before public checks pass.

## Failure behavior

SSH needs physical YubiKey presence. Prepare reviewable changes while access is
pending. If current server configuration conflicts with the bounded edit,
stop that branch and record the specific conflict. A failed health/proxy check
restores settings and routing; do not rotate secrets or restore active data.
