# Cloud and Deployment Policy

Local Docker Compose and the mock-backed server preview are implemented and approved. [infra/README.md](../../infra/README.md) describes local development. [SPEC-0010](../../specs/0010-backend-preview-deployment/spec.md) approves the separate `digesta-preview` stack on the existing server. [SPEC-0011](../../specs/0011-backend-dev-host/spec.md) corrects its API hostname to `dev.abako.xyz`. The backend is available at `https://dev.abako.xyz/api/`; the existing frontend remains in place. See the [testing and operations guide](digesta-backend-preview.md).

Tagged releases publish backend images to GHCR (see [release policy](release.md)). The preview uses the published `v0.2.0` digests; deployment is manual. There is no approved Kubernetes deployment, production database topology or automated promotion workflow.

The local gateway publishes loopback HTTP. Backend services use private networks, separate database mounts, selected secret files, non-root users and read-only root filesystems. Local HTTP cookies and development image tags are not production settings.

The server preview uses existing HTTPS, secure session cookies, exact allowed origins, private service ports and isolated stores. Its coherent backup was restored and verified in a separate stopped project. It uses simulated provider assets and development fixtures; it does not establish production custody or recovery policy.

Before production deployment, define and verify TLS, workload authentication, key management, image digests and supply-chain evidence, storage/recovery, monitoring and release/rollback procedures. HSM and envelope encryption have been discussed but are not integrated. Production deployment still requires the approval specified by AGENTS.md.
