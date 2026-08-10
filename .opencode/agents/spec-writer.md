---
description: Turns approved product intent into testable specifications and Gherkin acceptance scenarios.
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

Follow `agents/spec-writer.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `spec-driven-development`, `gherkin-specification`, `documentation-writing`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
