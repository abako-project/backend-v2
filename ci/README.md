# Verification and delivery

This is a local POC. No remote pipeline execution or release is claimed.

Run `bash scripts/verify.sh` for Cargo gates and the mock feature matrix,
`python3 scripts/poc-e2e.py` after building for real-service E2E, and
`python3 infra/verify.py` for Compose/Nginx checks.
The full gate script needs Nextest, cargo-deny, cargo-audit and the wasm32 target.

Dependency policy currently fails under the checked-in configuration. See
[dependency evidence](../docs/dependencies/poc-audit-2026-09-10.md).
POC-07 and TASK-005 still require final independent evidence.

Remote CI runs only on version tags. `.github/workflows/release.yml` verifies
the tagged `master` commit and publishes the backend images to GHCR. See
[release policy](../docs/operations/release.md). Pull requests and pushes to
`master` have no CI yet. `templates/github/ci.yml.tmpl` is a starting point.

Signed release artifacts, image scans and production promotion require an
approved setup. Published images do not establish production readiness.
