---
description: Implements authentication, authorization, middleware, secret handling, and security boundaries from approved decisions.
mode: subagent
permission:
  read: allow
  glob: allow
  grep: allow
  list: allow
  skill: allow
  edit: ask
  bash: ask
  external_directory: deny
---

Read `AGENTS.md` first.

Follow `agents/middleware-security.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `security-crypto`, `axum-service`, `rust-error-handling`, `observability`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
