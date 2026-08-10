---
name: model-selection
description: Select and verify coding-agent models without coupling project roles to vendor-specific model IDs. Use when configuring, upgrading, or comparing model providers for Codex, OpenCode, Claude Code, or Qwen Code.
compatibility: Codex, OpenCode, Claude Code, and Qwen Code
---

# Model Selection

Read `models/registry.json` and `docs/agentic/model-portability.md`.

- Keep model choice outside canonical role contracts.
- Verify a current model ID before pinning it.
- Keep credentials outside the repository.
- Prefer tool-managed authentication when available.
- Use representative project tasks to compare models.

A model upgrade should not require rewriting roles or skills.
