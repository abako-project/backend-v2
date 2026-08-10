# Task Graph

## Graph Rules

- Every task has one primary writer and explicit allowed paths.
- A task may start only when every declared dependency is complete.
- Shared root files and generated contracts are integration-owned unless explicitly assigned.
- Repeat failures without new evidence trigger a stop and handoff.

## Tasks

| ID | Role | Depends on | Allowed paths | Required skills | Completion gates | Human approval |
|---|---|---|---|---|---|---|
| TASK-001 | | | | | | |

## Parallel-Safe Groups

## Integration Order

## Rollback of Task-Level Changes
