# Contributing

## Workflow

1. Start from an approved specification.
2. Create one task issue with explicit dependencies and allowed paths.
3. Create one branch and worktree for the task.
4. Implement the smallest coherent change.
5. Run focused checks, then all applicable quality gates.
6. Write a reproducible handoff.
7. Open a pull request and request independent review.
8. Let the integration role merge in dependency order.

## Branch Names

`agent/<role>/<issue>-<slug>`

## Commits

Use small commits with an imperative Conventional Commit subject when practical. Do not mix unrelated refactors with feature behavior.

## Pull Requests

A pull request must identify the specification, requirements, scenarios, migration or contract effects, verification commands, risks, and rollback implications. No implementation agent approves its own work.
