# Task Graph

## Graph Rules

- Every task has one primary writer and explicit allowed paths.
- A task starts only when every dependency is complete.
- Shared root files and generated contracts belong to the integrator.
- Repeated failures without new evidence require a stop and handoff.
- All implementation remains blocked while `status.md` is not `APPROVED`.

## Tasks

| ID | Role | Depends on | Allowed paths | Required skills | Completion gates | Human approval |
|---|---|---|---|---|---|---|
| TASK-001 | architect | Approved SPEC-0001 | `contracts/custody/**` | serialization-contracts, security-crypto | Contract fixtures and compatibility tests pass | Required for signed contract |
| TASK-002 | middleware-security | TASK-001 | `services/wallet/**` | security-crypto, axum-service, rust-error-handling, rust-testing | Custody checks, encryption, and redaction tests pass | Required for crypto dependencies |
| TASK-003 | backend | TASK-001 | `services/adapter-api/**` | axum-service, tokio-concurrency, rust-error-handling, rust-testing | Provider-operation and custody-client tests pass | Not after contract approval |
| TASK-004 | backend | TASK-001 | `services/mock-provider/**` | axum-service, serialization-contracts, rust-testing | Signature, nonce, idempotency, and atomicity tests pass | Not after contract approval |
| TASK-005 | test-verifier | TASK-002, TASK-003, TASK-004 | Read-only repository; `progress/handoffs/SPEC-0001-verification.md` | gherkin-specification, rust-testing | Independent acceptance and security evidence recorded | No |
| TASK-006 | integrator | TASK-002, TASK-003, TASK-004, TASK-005 | `Cargo.toml`, `Cargo.lock`, `.env.example`, `infra/**`, `docs/dependencies/**`, `progress/handoffs/SPEC-0001.md` | rust-workspace-architecture, containerized-development, rust-quality | Workspace gates pass and local flow is reproducible | Required for dependency and integration review |

## Parallel-Safe Groups

- TASK-001 runs alone because it owns the shared contract.
- TASK-002, TASK-003, and TASK-004 may run in parallel after TASK-001.
- TASK-005 starts after all implementations are handed off.
- TASK-006 integrates after independent verification.

## Integration Order

1. Integrate the signed contract and fixtures.
2. Integrate custody, adapter provider worker, and mock provider without allowing them to edit shared root files.
3. Run independent verification.
4. Let the integrator update shared workspace, dependency, environment, and deployment files.
5. Run repository gates and record the final handoff.

## Rollback of Task-Level Changes

Each task uses its own branch and worktree. A failed task is omitted from integration; no task reverts another task's files. Contract corrections use a reviewed revision rather than consumer-specific patches.
