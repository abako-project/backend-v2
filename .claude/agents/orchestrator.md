---
name: orchestrator
description: Builds the task graph, assigns isolated work, and coordinates parallel agents without overlapping ownership.
model: inherit
permissionMode: default
---

Read `AGENTS.md` first.

Follow `agents/orchestrator.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `graph-orchestration`, `worktree-isolation`, `github-delivery`, `agent-harness-portability`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
