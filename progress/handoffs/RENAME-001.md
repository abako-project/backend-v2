# RENAME-001: Digesta project rename

Specification: `specs/0009-digesta-rename/spec.md` (APPROVED).
Primary writer: root agent, integrator.
Branch: `agent/integrator/RENAME-001-digesta`.
Task worktree: `/tmp/digesta-rename`.
Base commit: `c141d08a3885843fa0642facae60d1e05de3460f`.
Tested implementation commit: `23fc059c7ea8c452a93cacb24393181fc4a598b8`.
This handoff is a following documentation commit on the same branch.

## Scope and changed files

The specification assigns repository-wide name replacement to the sole
integrator, including shared contracts and workspace metadata. No dependency,
lockfile or migration changes were required.

```text
.env.example
.github/workflows/release.yml
.gitignore
CHANGELOG.md
Cargo.toml
README.md
apps/leptos-web/index.direct.html
apps/leptos-web/index.html
apps/leptos-web/src/browser.rs
apps/leptos-web/static/config.js
apps/leptos-web/tests/browser_smoke.py
contracts/openapi.json
crates/generated-contracts/API.md
crates/generated-contracts/src/lib.rs
crates/generated-contracts/tests/openapi.rs
docs/dependencies/bloque-client-2026-09-25.md
docs/operations/release.md
docs/project/digesta-rename.md
docs/project/project-context.md
infra/README.md
infra/compose.yaml
infra/frontend.Dockerfile
infra/nginx/config.js
infra/nginx/frontend.conf
infra/verify.py
migrations/README.md
scripts/poc-e2e.py
services/adapter-api/README.md
services/adapter-api/src/auth.rs
services/adapter-api/src/state.rs
services/adapter-api/src/tests.rs
services/mock-provider/tests/disputes/tests.rs
services/mock-provider/tests/transactions.rs
services/wallet/README.md
services/wallet/src/crypto.rs
services/wallet/src/tests.rs
specs/0001-custodial-wallet-signing/spec.md
specs/0001-custodial-wallet-signing/status.md
specs/0002-rest-frontend-isolation/spec.md
specs/0009-digesta-rename/acceptance.feature
specs/0009-digesta-rename/data-model.md
specs/0009-digesta-rename/design.md
specs/0009-digesta-rename/spec.md
specs/0009-digesta-rename/status.md
specs/0009-digesta-rename/tasks.md
specs/0009-digesta-rename/threat-model.md
progress/handoffs/RENAME-001.md
```

## Decisions and compatibility

- Inventory: 152 previous-name occurrences across 51 tracked files. Current
  product identifiers were renamed; retained occurrences are explained in
  `docs/project/digesta-rename.md`.
- The user selected the repository's current GitHub owner for GHCR. The release
  workflow derives and lowercases that owner and publishes `digesta-*` image
  names. The tag trigger, verification gates and image targets are retained.
- The session cookie is `digesta_session`; users must log in again. Cookie
  attributes, CSRF, allowed origins and authorization policies are unchanged.
- Browser configuration uses `DIGESTA_API_BASE` and `/etc/digesta/config.js`.
- New local setups use Digesta project/database defaults and
  `DIGESTA_LOCAL_DIR`. Existing environments need their original Compose
  project, database connection and secrets directory, as documented.
- Persisted cryptographic bytes were compared directly against the base and
  preserved. Historical evidence and the existing external-checkout ignore
  rule remain accurate.
- Compose verification originally omitted PostgreSQL; reproduced against the
  base revision, then corrected to validate the current six-service stack.

## Commands and observed results

Cargo commands used `RUSTUP_TOOLCHAIN=1.96.1`. Native dependency builds used
the existing cache with `--offline`; no dependency resolution changes occurred.

| Command/check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --all-features --locked --offline` | PASS; `/tmp/digesta-check.log` |
| `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` | PASS; `/tmp/digesta-clippy.log` |
| `cargo build --workspace --all-targets --all-features --locked --offline` | PASS; `/tmp/digesta-build.log` |
| `cargo test --workspace --doc --locked --offline` | PASS; `/tmp/digesta-doc.log` |
| `cargo test --workspace --all-features --exclude adapter-api --locked --offline` | PASS: 74 tests with sandbox escalation for disposable loopback sockets; `/tmp/digesta-tests-loopback.log` |
| `cargo test --workspace --all-features --locked --offline` | Blocked in sandbox: adapter had 10 passing tests and 18 failures from unavailable PostgreSQL configuration or denied loopback binds; `/tmp/digesta-tests.log` |
| `cargo nextest run --workspace --all-features` | Unavailable: Nextest not installed |
| `cargo audit` | Unavailable: cargo-audit not installed |
| `cargo deny --offline check` | Blocked: all-platform metadata needs uncached packages and registry extraction in the read-only Cargo cache; `/tmp/digesta-deny.log` |
| `cargo check -p leptos-web --target wasm32-unknown-unknown --locked --offline` | Blocked: pinned 1.96.1 toolchain lacks WASM standard library; `/tmp/digesta-wasm.log` |
| `python3 infra/verify.py --config-only` | PASS: isolation, renamed defaults/images, external frontend override |
| OpenAPI JSON and release YAML checks | PASS: Digesta API/cookie, version tag filter, verify dependency, three push-enabled backend targets |
| `node --check` on both config.js files; Python AST checks | PASS |
| Namespace shell step with mixed-case owner | PASS: `ghcr.io/abako-project/digesta-` |
| Current-name scan, crypto-byte comparison, config/mount consistency, `git diff --check` | PASS |

The first non-PostgreSQL test attempt hit sandbox loopback restrictions. The
successful rerun has a separate clean log. Docker image inspection outside the
sandbox confirmed that no daemon socket is available, so Nginx/container smoke
tests could not run. PostgreSQL executables are absent as well.

## Covered scenarios and limits

NAME-001 through NAME-005 are implemented. OpenAPI cookie and persisted-format
compatibility checks pass. Publishing configuration is verified locally; no
release was triggered and no package was published. Real adapter login and
PostgreSQL integration require the unavailable test database.

No GitHub ownership, registry packages, deployed containers, live databases,
private secrets, Git remote or external checkout was changed. Existing
environments must follow the rename notes before switching local defaults.

## Blocker

Full verification needs PostgreSQL with `TEST_ADAPTER_DATABASE_URL`, Nextest,
cargo-audit, complete Cargo metadata/cache access, the WASM target for Rust
1.96.1, and a Docker daemon. These environment limits remain open; the rename
does not claim all project gates passed.

