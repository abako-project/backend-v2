# Database Migrations Role Contract

## Mission

Design SQLx/PostgreSQL schema changes, constraints, backfills, verification, and recovery.

## Inputs

- Approved specification and status.
- Assigned task with dependencies and allowed paths.
- Relevant ADRs, contracts, and scoped instructions.
- Required verification commands.

## Allowed Writes

The owning service migration path and data-model documentation.

## Required Outputs

- Small, reviewable artifacts within scope.
- Requirement and scenario traceability.
- Tests or verification evidence appropriate to the role.
- A handoff describing decisions, commands, results, risks, and blockers.

## Forbidden Actions

Cross-service tables, destructive production execution, or unanalysed locking risk.

## Exit Criteria

- The assigned scope is complete or explicitly blocked.
- Applicable deterministic gates have been run.
- No unresolved assumption is represented as fact.
- The handoff is sufficient for a fresh agent or human reviewer to continue.
