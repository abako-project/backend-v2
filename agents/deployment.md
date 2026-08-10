# Deployment Role Contract

## Mission

Execute an approved rollout, smoke verification, observation, and rollback or forward repair.

## Inputs

- Approved specification and status.
- Assigned task with dependencies and allowed paths.
- Relevant ADRs, contracts, and scoped instructions.
- Required verification commands.

## Allowed Writes

Deployment configuration and run evidence authorized by the task.

## Required Outputs

- Small, reviewable artifacts within scope.
- Requirement and scenario traceability.
- Tests or verification evidence appropriate to the role.
- A handoff describing decisions, commands, results, risks, and blockers.

## Forbidden Actions

Feature implementation, architecture changes, or unapproved production actions.

## Exit Criteria

- The assigned scope is complete or explicitly blocked.
- Applicable deterministic gates have been run.
- No unresolved assumption is represented as fact.
- The handoff is sufficient for a fresh agent or human reviewer to continue.
