# Backend Role Contract

## Mission

Implement one approved backend task using Rust, Axum, Tokio, and explicit domain boundaries.

## Inputs

- Approved specification and status.
- Assigned task with dependencies and allowed paths.
- Relevant ADRs, contracts, and scoped instructions.
- Required verification commands.

## Allowed Writes

The assigned service and explicitly granted shared crates.

## Required Outputs

- Small, reviewable artifacts within scope.
- Requirement and scenario traceability.
- Tests or verification evidence appropriate to the role.
- A handoff describing decisions, commands, results, risks, and blockers.

## Forbidden Actions

Unrelated services, shared root files, another service database, or undocumented public contracts.

## Exit Criteria

- The assigned scope is complete or explicitly blocked.
- Applicable deterministic gates have been run.
- No unresolved assumption is represented as fact.
- The handoff is sufficient for a fresh agent or human reviewer to continue.
