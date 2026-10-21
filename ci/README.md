# Verification and delivery

This is a mock-backed development backend. Tagged releases have a verification
and image-publication workflow; they do not establish production readiness.

Run `bash scripts/verify.sh` for Cargo gates and the mock feature matrix,
`python3 scripts/poc-e2e.py` after building for real-service E2E, and
`python3 infra/verify.py` for Compose/Nginx checks.
The full gate script needs Nextest, cargo-deny, cargo-audit and the wasm32 target.

Dependency policy currently fails under the checked-in configuration. See
[dependency evidence](../docs/dependencies/poc-audit-2026-09-10.md).
POC-07 and TASK-005 have historical local acceptance evidence; see
[project status](../docs/project/project-context.md).

Remote CI runs only on version tags. `.github/workflows/release.yml` verifies
the tagged `master` commit and publishes the backend images to GHCR. See
[release policy](../docs/operations/release.md). Pull requests and pushes to
`master` have no CI yet. The approved simplification plan adds shared PR checks
in milestone 3.

Signed release artifacts, image scans and production promotion require an
approved setup. Published images do not establish production readiness.
