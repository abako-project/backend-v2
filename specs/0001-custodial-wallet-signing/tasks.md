# Task Graph

Status update 2026-09-15: implementation commits are integrated into
`feat/rust-rest-poc` and temporary implementation worktrees are removed. TASK-005
remains open; see `status.md` for missing acceptance/security evidence and the
rotation scope discrepancy. Paths below describe task ownership, not existing
verification reports. Current human test commands do not require RTK.

## Graph Rules

- Every task has one primary writer and explicit allowed paths.
- A task starts only when every dependency is complete.
- Shared root files and generated contracts belong to the integrator.
- Repeated failures without new evidence require a stop and handoff.
- All implementation remains blocked while `status.md` is not `APPROVED`.

## Tasks

| ID | Role | Depends on | Allowed paths | Required skills | Completion gates | Human approval |
|---|---|---|---|---|---|---|
| TASK-000 | integrator | Approved SPEC-0001 | `Cargo.toml`, `Cargo.lock`, required crate manifests, `docs/dependencies/**` | rust-workspace-architecture, rust-dependency-selection | Compilable workspace and reviewed dependency baseline available in task worktrees | Required for new cryptographic/native dependencies |
| TASK-001 | architect | TASK-000 | `contracts/custody/**`, assigned shared wire crate | serialization-contracts, security-crypto | Contract fixtures and compatibility tests pass | Required for signed contract |
| TASK-002 | middleware-security | TASK-001 | `services/wallet/**` | security-crypto, axum-service, rust-error-handling, rust-testing | Custody checks, encryption, and redaction tests pass | Required for crypto dependencies |
| TASK-003 | backend | TASK-001 | `services/adapter-api/**` | axum-service, tokio-concurrency, rust-error-handling, rust-testing | Provider-operation and custody-client tests pass | Not after contract approval |
| TASK-004 | backend | TASK-001 | `services/mock-provider/**` | axum-service, serialization-contracts, rust-testing | Signature, nonce, idempotency, and atomicity tests pass | Not after contract approval |
| TASK-005 | test-verifier | TASK-006 | Read-only repository; `progress/handoffs/SPEC-0001-verification.md` | gherkin-specification, rust-testing | Independent acceptance and security evidence on the integrated tree | No |
| TASK-006 | integrator | TASK-002, TASK-003, TASK-004 | `Cargo.toml`, `Cargo.lock`, `.env.example`, `infra/**`, `docs/dependencies/**`, `progress/handoffs/SPEC-0001.md` | rust-workspace-architecture, containerized-development, rust-quality | Integrated workspace builds and local flow is reproducible | Required for dependency and integration review |

## Parallel-Safe Groups

- TASK-000 establishes the workspace before TASK-001 produces the shared contract.
- TASK-002, TASK-003, and TASK-004 may run in parallel after TASK-001.
- TASK-006 integrates all implementation handoffs.
- TASK-005 independently verifies the integrated tree.

## Integration Order

1. Establish the compilable workspace and dependency baseline.
2. Integrate the signed contract and fixtures.
3. Implement custody, adapter provider worker, and mock provider in parallel without shared-root edits.
4. Integrate implementations, environment, and deployment files.
5. Run independent verification and repository gates on the integrated tree, then record the final handoff.

## Rollback of Task-Level Changes

Each task uses its own branch and worktree. A failed task is omitted from integration; no task reverts another task's files. Contract corrections use a reviewed revision rather than consumer-specific patches.
