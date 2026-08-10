---
description: Reviews changes against the specification, contracts, Rust quality rules, security boundaries, and evidence.
mode: subagent
permission:
  read: allow
  glob: allow
  grep: allow
  list: allow
  skill: allow
  edit: deny
  bash: ask
  external_directory: deny
---

Read `AGENTS.md` first.

Follow `agents/reviewer.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `rust-quality`, `rust-testing`, `security-crypto`, `performance-engineering`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
