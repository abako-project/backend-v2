# Verification and CI

Run the same application checks locally and in GitHub Actions:

```sh
bash scripts/install-dev-tools.sh
bash scripts/verify.sh app
```

## Prerequisites

- The pinned Rust 1.96.1 toolchain, rustfmt, Clippy and the
  `wasm32-unknown-unknown` target. The installer sets up those components.
- Python 3 and PostgreSQL server tools `initdb`, `pg_ctl` and `psql` on `PATH`.
  On Debian/Ubuntu, install `postgresql` and add its versioned server bin
  directory (for example `/usr/lib/postgresql/17/bin`) to `PATH`.
- A non-root user able to start local processes and bind loopback test ports.
  Tests use temporary directories; no existing application database or secret
  directory is needed.

Application verification runs formatting, workspace check, strict Clippy,
workspace tests, doctests, the mock's isolated memory/SQLite feature combinations,
workspace build, frontend WASM check and the real-service E2E in both mock modes.
It uses Cargo's built-in test runner; Nextest is not a prerequisite.

`scripts/test_postgres.py` starts disposable PostgreSQL for workspace tests and
stops it after the command. An explicit `TEST_ADAPTER_DATABASE_URL` can select an
existing **test** database instead; tests create isolated schemas there. E2E
always starts its own disposable PostgreSQL and real services. Both helpers
remove their temporary data and require local socket access. `CARGO_TARGET_DIR`
is respected when selecting the E2E binaries.

To run only backend E2E after building:

```sh
cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked
python3 scripts/poc-e2e.py
```

These are API checks. Browser behavior and the external frontend require a
separate walkthrough.

## ink! v5 compatibility proof

```sh
bash scripts/verify.sh ink-sandbox
```

This separate native workspace checks the pinned v5 WASM against the proposed
FRAME runtime. It runs formatting, check, strict Clippy, tests and build with its
own lockfile. It also exercises the shared signed-operation shell against real
WASM fixtures. It needs neither PostgreSQL nor a node nor cargo-contract at test
runtime. The path-filtered `ink! v5 compatibility` workflow runs it only when the
proof or its verification command changes; normal app builds keep their current
dependency graph. It does not establish application parity or enable a provider
switch. Source rebuilds use the pinned contract toolchain separately; see the
[proof guide](../tools/ink-sandbox/README.md).

## Container and gateway checks

With a running Docker daemon accessible to your user:

```sh
bash scripts/verify.sh containers
```

This checks Compose isolation, the independent frontend override, Nginx routing
and SSE transport, then builds all three backend image targets as local
`:verification` tags. It publishes nothing and reads no application secrets.
To check Compose rendering alone without a daemon:

```sh
python3 infra/verify.py --config-only
```

See the [Compose guide](../infra/README.md) for the full stack and browser image.

## Dependency policy and optional tools

```sh
bash scripts/install-dev-tools.sh --security
bash scripts/verify.sh dependencies
```

The dependency command runs both cargo-deny and cargo-audit and fails if either
fails or is unavailable. These checks remain separate from the application gate:
existing license/advisory-policy findings are documented in
[supply-chain status](../docs/security/supply-chain.md). Application verification
passing does not mean the dependency policy passes.

Coverage (`--coverage`) and nightly/Miri (`--miri`) are opt-in installer choices.
Their installation does not run those checks or establish coverage evidence.

## GitHub Actions and delivery

[Verify](../.github/workflows/verify.yml) runs `scripts/verify.sh app` for pull
requests and pushes to `master`. It is also the reusable verification job called
by [Release](../.github/workflows/release.yml). CI installs native PostgreSQL
server tools and the same pinned Rust toolchain/target; it requires no agent
bootstrap or model credentials. Third-party actions remain SHA-pinned, checkout
credentials are not persisted, and verification has read-only repository access.

Version tags still require the tagged commit to belong to `master`. Only after
shared verification succeeds does Release publish the three backend images.
Image names and tag conventions are unchanged; see [release policy](../docs/operations/release.md).
The container/gateway and dependency-policy commands are explicit separate checks,
not silently included in the PR application gate.

Record the tested commit, commands, results and limitations when reporting
verification. Signed images, SBOMs and production promotion are not established
by these workflows.

SDK integration has a separate owner-approved, exact-lock dependency check:

```sh
bash scripts/verify.sh ink-sandbox-dependencies
```

The SDK policy expires on 2026-12-01 and accepts only its recorded package/version
licenses and advisory IDs. A changed SDK lock requires a new review. The root
application policy and its existing findings remain unchanged.

The SDK workflow runs for changes anywhere in `services/mock-provider/`, because
its manifest, HTTP boundary, reference and storage/read models are linked by the
candidate. It installs the already reviewed cargo-deny 0.20.2 with its locked
tool dependencies and runs the policy wrapper before compatibility tests. This
checks the exact SDK lock, review expiry, current advisories, licenses and sources;
it does not grant exceptions to the root application policy. The tool's published
metadata declares Rust 1.88.0 and MIT OR Apache-2.0; it is a CI checker and adds no
application or SDK dependency. Direct cargo-deny without the wrapper would skip
the approved fingerprint and expiry checks.
