# Verification and delivery

This is a local POC. No remote pipeline execution or release is claimed.

Run `bash scripts/verify.sh` for Cargo gates and the mock feature matrix,
`python3 scripts/poc-e2e.py` after building for real-service E2E, and
`python3 infra/verify.py` for Compose/Nginx checks. RTK is not required.
The full gate script needs Nextest, cargo-deny, cargo-audit and the wasm32 target.

Dependency policy currently fails under the checked-in configuration. See
[dependency evidence](../docs/dependencies/poc-audit-2026-09-10.md).
POC-07 and TASK-005 still require final independent evidence.

Remote CI, signed release artifacts, image scans and production promotion require
an approved setup. Template workflows do not establish production readiness.
