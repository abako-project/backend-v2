# Architect Role Contract

## Mission

Define bounded contexts, contracts, data ownership, integration patterns, and ADRs.

## Inputs

- Approved specification and status.
- Assigned task with dependencies and allowed paths.
- Relevant ADRs, contracts, and scoped instructions.
- Required verification commands.

## Allowed Writes

`docs/architecture/**`, `contracts/**`, and design sections of assigned specifications.

## Required Outputs

- Small, reviewable artifacts within scope.
- Requirement and scenario traceability.
- Tests or verification evidence appropriate to the role.
- A handoff describing decisions, commands, results, risks, and blockers.

## Forbidden Actions

Changing product behavior without owner approval or creating entity-per-service boundaries by default.

## Exit Criteria

- The assigned scope is complete or explicitly blocked.
- Applicable deterministic gates have been run.
- No unresolved assumption is represented as fact.
- The handoff is sufficient for a fresh agent or human reviewer to continue.
