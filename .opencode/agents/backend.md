---
description: Implements approved Rust backend behavior with Axum, Tokio, contracts, and bounded service ownership.
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

Follow `agents/backend.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `rust-quality`, `rust-error-handling`, `tokio-concurrency`, `axum-service`, `graphql-contract`, `grpc-tonic`, `realtime-channel`, `performance-engineering`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
