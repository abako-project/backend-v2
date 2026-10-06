# Release Policy

This repository is a local POC. Its integrated commits and test handoffs are development evidence, not production releases. Published images are a delivery convenience; they do not make the backend production-ready.

Before marking a feature complete, run its acceptance checks and applicable AGENTS.md gates and record the command results against the tested revision. POC-07 and TASK-005 have independent handoffs for local mock-backed POC acceptance; custody recovery and dependency-policy gaps still bar production claims.

## Container images

Pushing a tag that matches `v*.*.*` runs `.github/workflows/release.yml`:

1. **verify** fails unless the tagged commit is on `master`. It then runs fmt, clippy, Nextest (against a PostgreSQL service) and doc tests with Rust 1.96.1, the same compiler `infra/backend.Dockerfile` uses.
2. **publish** builds the `adapter-api`, `wallet` and `mock-provider` targets of `infra/backend.Dockerfile` and pushes them to GHCR. It runs only if verify passes.

```sh
git switch master && git pull --ff-only
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

| Image | Source target |
|---|---|
| `ghcr.io/abako-project/kunveno-adapter-api` | `adapter-api` |
| `ghcr.io/abako-project/kunveno-wallet` | `wallet` |
| `ghcr.io/abako-project/kunveno-mock-provider` | `mock-provider` |

Each release gets these tags. `v1.4.2` is shown as an example:

| Tag | Moves? | Notes |
|---|---|---|
| `1.4.2` | no | Exact release. Pin this or the digest for reproducible deploys. |
| `1.4` | yes | Latest patch of the minor line. |
| `1` | yes | Latest release of the major line. Not published for `v0.x`. |
| `latest` | yes | Newest non-prerelease. |

A prerelease such as `v1.5.0-rc.1` gets only its exact tag. It never moves `latest` or a release line.

`cargo deny check` and `cargo audit` are not release gates yet, because the dependency policy currently fails under the checked-in configuration. See [dependency evidence](../dependencies/poc-audit-2026-09-10.md). Signed images, published SBOMs, vulnerability scans and promotion/rollback automation are not implemented.

### GHCR access

The workflow pushes with the job's `GITHUB_TOKEN`, so no repository secret is needed. The first push creates each package as **private** in the `abako-project` organization. Hosts that pull a private package need `podman login ghcr.io` with a token that has `read:packages`. Package visibility is set in the organization's package settings, not in the workflow.

### Podman auto-update

Published images carry the label `io.containers.autoupdate=registry`. Podman copies image labels onto the containers it creates, so a container started from one of these images opts in to auto-update. `podman auto-update` then pulls the container's tag again and restarts the systemd unit when the registry digest changes. Two conditions apply:

- The container must run under a systemd unit, for example a Quadlet `.container` file.
- `Image=` must be a fully qualified reference that names a moving tag. A container pinned to `1.4.2` never changes.

A minimal Quadlet unit, for example `~/.config/containers/systemd/kunveno-wallet.container`:

```ini
[Container]
Image=ghcr.io/abako-project/kunveno-wallet:0.2
# Optional: the image label already opts in. Set AutoUpdate= here to make it explicit or to override it.
AutoUpdate=registry
# Environment, secrets, volumes and networks: mirror the wallet service in infra/compose.yaml.

[Install]
WantedBy=default.target
```

```sh
systemctl --user daemon-reload
systemctl --user start kunveno-wallet.service
systemctl --user enable --now podman-auto-update.timer
podman auto-update --dry-run
```

To stop one container from following updates, set `AutoUpdate=disabled` in its unit, or use `--label io.containers.autoupdate=disabled` with `podman run`. Container labels override image labels. An empty `AutoUpdate=` does not opt out: Quadlet skips the key, so the image label still applies.

## Not a production release

Cargo.lock records application dependency resolution. Docker build inputs are documented in [infra/README.md](../../infra/README.md). Locally built `:local` tags are not release identities.

A production release still needs a chosen versioning and compatibility policy for REST and signed formats, resolved dependency policy, recovery evidence, image signing and supply-chain evidence, and explicit release approval.

## References

- [podman-auto-update(1)](https://docs.podman.io/en/latest/markdown/podman-auto-update.1.html)
- [podman-systemd.unit(5)](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html)
- [docker/metadata-action semver tags](https://github.com/docker/metadata-action#typesemver)
- [Publishing packages to GHCR with GITHUB_TOKEN](https://docs.github.com/en/packages/managing-github-packages-using-github-actions-workflows/publishing-and-installing-a-package-with-github-actions)
