# TASK-005: Independent custody acceptance and security verification

Specification: SPEC-0001 (APPROVED).  
Task: TASK-005.  
Role: test-verifier.  
Branch: `feat/port-verification`.  
Worktree: `/tmp/kunveno-port-verification`.  
Base commit: `a8d2388` (runtime implementation is unchanged from `a5e0d85`).

## Allowed paths and files changed

Only this report and `POC-07-verification.md`; no implementation, test or contract changes.

## Observed evidence

- Exact-revision all-feature service build exited 0. Rust 1.96.1 printed a `rustc interrupted by SIGSEGV` LLVM debug-location backtrace during dependency compilation despite the successful Cargo exit; it did not appear in the subsequent E2E run.
- The independently run signed E2E passed all eight SQLite/memory scenarios with disposable PostgreSQL at `9435897`. It covers one wallet per principal, password change preserving `AccountId32`, authenticated typed operations, signed execution, idempotent replay after a dropped reply, project authorization, and event/SSE recovery. Raw REST bodies, SSE data lines, custody metrics and bounded service logs were scanned for generated secret markers.
- At `a8d2388`, the scan was moved before parsing every SSE line; the exact SSE happy-path case independently passed again for both SQLite and memory. No runtime implementation changed between these revisions.
- `cargo test -p wallet -p mock-provider --all-features --locked` passed 39 focused tests twice. These cover encryption binding, invalid payloads, signatures, nonces, leases, lifecycle, authorization, duplicate calls, atomic rollback and both mock backends. The compiler backtrace also appeared before these successful test runs.
- The custody HTTP test checks unauthorized error bodies and `/internal/metrics` for a service-token marker. Public custody DTOs contain wallet identifiers, status and signature, not ciphertext or seed; custody errors serialize stable codes. The E2E scans all raw REST bodies it receives, every SSE line, authenticated custody metrics, and bounded stdout/stderr from wallet, mock and adapter for generated master-key, root-seed, service-token and bootstrap-password markers. Service tracing is emitted to captured stderr.
- `cargo fmt --all -- --check` and `git diff --check` passed.

## Decision and limits

TASK-005 can close for the **local mock-backed POC**. The independent run now
covers the specified known-marker response and telemetry surfaces as well as
the signed lifecycle and focused negative tests. It does not establish that
every possible secret is absent or that the system is ready for production.

The approved `81da8d5` clarification makes HSM-backed encryption-key rotation a
**pre-real-value requirement**, not a POC TASK-005 gate. Current ciphertext uses
the POC master key; there is no HSM, DEK/KEK wrapping, online rotation, backup
recovery, or protection against full custody-host compromise. No real assets
may be entrusted to this stack.

The marker scan covers the four generated fixture secrets, not every runtime
seed or session credential; logs are limited to 4 MiB per service. The first
run against pre-existing binaries of unverified feature/revision match failed
the catalog-seed assertion;
rebuilding all three service binaries with `--all-features` removed the failure.
The repeated `rustc` SIGSEGV diagnostic despite Cargo exit 0 needs separate
toolchain investigation before treating this environment's build output as
clean. A separate integrator rerun of the 39 focused tests at `a8d2388` passed
without that diagnostic. The owner excluded Leptos and cargo-deny from this
backend POC; neither was run, and this report makes no dependency-policy or
production-security claim.

Blocker for TASK-005 within the approved local POC scope: none.
