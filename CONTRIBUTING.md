# Contributing

## Workflow

1. Start from an approved specification.
2. Define one local task ID with explicit dependencies and allowed paths. Remote issues require an approved remote workflow.
3. Create one branch and worktree for the task.
4. Implement the smallest coherent change.
5. Run focused checks, then all applicable quality gates.
6. Write a reproducible handoff.
7. Request independent local review; use a pull request only after a remote workflow is authorized.
8. Let the integration role merge in dependency order.

## Branch Names

`agent/<role>/<issue>-<slug>`

## Commits

Use small commits with an imperative Conventional Commit subject when practical. Do not mix unrelated refactors with feature behavior.

## Pull Requests

A pull request must identify the specification, requirements, scenarios, migration or contract effects, verification commands, risks, and rollback implications. No implementation agent approves its own work.

## Human commands and agent tools

Users and agents run Cargo, Docker and Python directly. RTK is disabled until the
user requests it again; it is not a build or deployment dependency. Sandbox prompts
must identify the restricted resource (Docker, sockets, downloads or Git metadata).
