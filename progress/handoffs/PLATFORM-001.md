# PLATFORM-001: Public platform profile

Date: 2026-10-21. Status: implementation complete; functional verification passed.

Spec: `specs/0012-platform-profile/` (APPROVED).
Branch: `agent/integrator/PLATFORM-001-platform-profile`.
Worktree: `/tmp/digesta-platform-profile`. Base: `5f8cfb4`.
Primary writer: root, integrator.

## Changes and decisions

- Added shared typed platform DTOs, a bundled software/Abako profile, bounded
  optional `PLATFORM_CONFIG_PATH` loading and public GET/HEAD `/api/platform`.
- Default currency decimals is 0, matching the current mock's integer units.
  The owner authorized a sensible value. No amounts or provider rules changed.
- Project briefs accept exact configured labels plus all six legacy values.
  Historical configured types remain readable after profile changes. The
  adapter still owns brief JSONB; no SQL migration or dependency was added.
- Updated OpenAPI, public-route contract assertions, HTTP documentation and
  focused startup/HTTP/brief compatibility tests. Test config construction
  now propagates profile-loading failures instead of assuming a valid fixture.
- Added public-profile and configured Full Stack checks to the signed-service
  E2E. Corrected its stale coordinator reservation assertion from 160 to the
  fixture's declared 60 minutes, consistent with approved SPEC-0003. Updated
  the exact private-profile fixture to include existing nullable contactEmail.

Changed scope: specification artifacts; generated-contracts platform module,
export, public contract tests and API documentation; adapter platform module,
router/state/module wiring, project-brief models/validation and test fixtures;
OpenAPI; adapter README; scripts/poc-e2e.py; this handoff.

## Verification

Use Rust 1.96.1. This host's default Cargo is older, so commands used the
installed 1.96.1 toolchain bin directory first in PATH and the shared existing
Cargo target directory. Dependencies were resolved with --locked --offline.

Docker has no running daemon. Native PostgreSQL binaries are absent from PATH;
a cached PostgreSQL 17.11 image provided executable tools and their matching
libraries/ICU data. A disposable trust-authenticated loopback-only cluster under
/tmp enabled actual database tests without modifying the host installation.
The temporary harness `/tmp/digesta-platform-verify.py` started/stopped that
cluster for the ordinary commands below. Its final invocation reran only E2E
after the Rust gates passed. Sandbox escalation allowed temporary local sockets.

| Command | Observed result |
|---|---|
| cargo fmt --all -- --check | PASS |
| cargo check --workspace --all-targets --all-features --locked --offline | PASS |
| cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings | PASS |
| cargo test --workspace --all-features --locked --offline | PASS: 106 tests, including 32 adapter tests against PostgreSQL |
| cargo test --workspace --doc --locked --offline | PASS |
| cargo build --workspace --all-targets --all-features --locked --offline | PASS |
| python3 -u scripts/poc-e2e.py | PASS: all eight scenarios across SQLite and memory |
| Python AST, OpenAPI references and complete supplied-profile comparison | PASS |
| cargo nextest run --workspace --all-features | Unavailable: Nextest is not installed; cargo test used instead |
| cargo audit | Unavailable: cargo-audit is not installed |
| cargo deny --offline check | Blocked: uncached etcetera 0.11.0 prevents all-platform metadata resolution |

Logs: `/tmp/digesta-platform-{fmt,check,clippy,test,doc,build,e2e,nextest,audit,deny}.log`.
No passing audit/deny or container-build claim is made. The temporary target
symlink used by the E2E default binary path was removed after verification.

## Covered behavior and limits

Anonymous profile GET and HEAD; no database/provider dependency at request time;
allowed/disallowed Origin; no session cookie issued; default full JSON shape;
alternative domain configuration; bounded and malformed configuration rejection;
configured and legacy brief writes; rejection of unconfigured types; preserved
historical types after adapter restart/profile change; no money conversion.

An initial OpenAPI test failed because it enumerated all public routes and
expected 95 operations; the additive route and 96-operation count are now
explicitly checked. An initial real-service E2E exposed the stale planning-era
capacity expectation described above. A later auxiliary fixture expected the
private client DTO without its current nullable contactEmail field; the fixture
now includes that field. Application scheduling and profile code were unchanged.

## Delivery and recovery

No image was published, deployment performed or active data reset. The feature
was implemented in this isolated worktree and is ready for local fast-forward
integration. The bundled profile needs no new deployment file.
To customize it, mount a public JSON file read-only, set PLATFORM_CONFIG_PATH to
its in-container path and restart all adapter replicas with matching profiles.

Metadata adds no state. After saving a new custom brief type, an older enum-only
adapter cannot read that value; use a compatible forward release or reviewed
migration when planning rollback. Private external-profile persistence, DAO
selection/multi-tenancy, real currency metadata lookup and chain integration
remain outside scope. Existing GitHub/profile fields are unchanged.
