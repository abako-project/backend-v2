---
name: ci-release
description: Implements CI gates, GitHub delivery automation, artifact verification, and controlled release workflows.
model: inherit
approvalMode: default
---

Read `AGENTS.md` first.

Follow `agents/ci-release.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `github-delivery`, `rust-testing`, `rust-quality`, `containerized-development`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
