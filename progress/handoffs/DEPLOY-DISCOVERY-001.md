# DEPLOY-DISCOVERY-001: Server inspection and deployment proposal

Task: Inspect the user-selected server and propose a parallel Digesta deployment.

Primary writer: root agent, integrator (documentation only).

Branch: `agent/integrator/DEPLOY-DISCOVERY-001-proposal`.

Worktree: `/tmp/digesta-deployment-proposal`.

Base commit: `bdbbaa8e770f83b9a636f6fec75b130d8f713e54` (`v0.2.0`).

Allowed paths: `docs/operations/digesta-server-proposal.md` and this handoff.

Relevant authority: AGENTS.md, SPEC-0001 custody, SPEC-0002 REST/frontend isolation,
ADR-0001, and `docs/operations/cloud.md`. No approved remote deployment
specification exists; the requested output is a DRAFT proposal.

## Files and decisions

- `docs/operations/digesta-server-proposal.md`: observed host, existing routes,
  data locations, verified release digests, recommended topology, rollout,
  validation and rollback.
- This handoff: authorization, inspection evidence and implementation boundary.

The user selected SSH alias `digesta`, chose a parallel rollout before switching
traffic, and requested a `digesta.*` subdomain of the existing domain.
The proposal uses `digesta.dev.abako.xyz`. Inspection found no DNS record for it.

Recommend the existing Docker Compose/host Nginx model, separate new state,
loopback adapter port 4080 and a compatible frontend. Existing static frontend
assets call legacy authentication/payment routes and cannot be switched with
only a backend URL change. Existing Abako state remains separate.

## Commands and observed results

- `ssh -G digesta`: root at `128.140.7.148:22`, configured security-key identity.
  Sandbox system-config ownership validation needed escalation.
- First SSH signing attempts failed. A diagnostic retry established that the
  server accepts the public key but FIDO user presence timed out. The user
  confirmed readiness; the next attempt authenticated with a physical touch.
  Password fallback was disabled after the first attempt.
- SSH multiplexing reused that authenticated connection for follow-up reads;
  no agent forwarding was used.
- Read-only `cat /etc/os-release`, `uname -m`, `nproc`, `free -h`, `df -hT`,
  `systemctl`, `ss -lnt`, Docker version/ps/filtered inspect, UFW status and
  directory metadata: Ubuntu 24.04.3, x86-64, 8 CPUs, 15 GiB memory, 147 GiB
  free storage; Docker/Compose and host Nginx already run the legacy environment.
- Filtered `nginx -T`: public server names, upstream ports and served static
  frontend path. Only route/storage/public TLS fields were returned; secret
  headers and private key content were excluded.
- Public `openssl x509 -noout -dates -subject`: current certificates cover
  `dev.abako.xyz` and `web.dev.abako.xyz`.
- Frontend manifest and public bundle route literals: React/Vite legacy source;
  public release `/opt/abako/web-dev/releases/f4b1ec5` contains legacy routes.
- User-scoped Podman queries failed due user-session/working-directory setup;
  root Podman showed no containers. The proposal uses the observed Docker stack.
- DNS lookup: existing hostnames resolve to `128.140.7.148`; proposed hostname
  does not resolve. A reboot-required marker exists; no reboot was performed.
- `gh run watch 38712832177 --exit-status`: success. Verify took 5m13s and
  publish took 4m10s. All release tests and three image pushes passed.
- Package metadata API was unavailable without `read:packages` scope. An
  anonymous GHCR token/manifest probe independently fetched all three `0.2.0`
  manifests, checked digests against publication logs, and confirmed `linux/amd64`
  platform support. No token values were output. These images need no pull login.
- Primary guidance reviewed: Docker Compose production, GHCR container registry,
  Certbot webroot and Podman Quadlet documentation. Used references are linked
  next to the relevant proposal recommendations.

## Verification and limits

Documentation-only change: applicable local checks are `git diff --check`, review
of internal links, and comparing the report with observed command results.
No Rust files or deployment scripts changed, so Rust gates are not applicable.

The host was inspected without installing software, pulling server images,
changing Nginx/DNS/certificates, restarting services, modifying application data
or reading secrets into output. The temporary SSH multiplexing connection is
closed after inspection.

No real capacity test, backup restore, deployment or cutover was performed.
External/provider backups were not inspected. The candidate frontend artifact,
DNS ownership/access, any requested legacy data migration, and the cutover/write
policy must be resolved in deployment implementation.

## Blocker

None for the requested investigation and proposal. Applying the plan is a
separate task, requiring an approved deployment specification and authorization
for server, DNS and routing changes.
