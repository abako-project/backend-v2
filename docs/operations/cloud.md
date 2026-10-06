# Cloud and Deployment Policy

Only local Docker Compose is implemented and approved. [infra/README.md](../../infra/README.md) describes it. There is no approved cloud provider, Kubernetes deployment, production database topology or automated promotion workflow. Tagged releases publish backend images to GHCR (see [release policy](release.md)), but nothing deploys them.

The local gateway publishes loopback HTTP. Backend services use private networks, separate database mounts, selected secret files, non-root users and read-only root filesystems. Local HTTP cookies and development image tags are not production settings.

Before production deployment, define and verify TLS, workload authentication, key management, image digests and supply-chain evidence, storage/recovery, monitoring and release/rollback procedures. HSM and envelope encryption have been discussed but are not integrated. Production deployment still requires the approval specified by AGENTS.md.
