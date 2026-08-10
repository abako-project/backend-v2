---
name: agent-harness-portability
description: Maintain one canonical set of agent roles and skills across Codex, OpenCode, Claude Code, and Qwen Code. Use when changing agent adapters, discovery paths, permissions, or shared skill wiring.
compatibility: Codex, OpenCode, Claude Code, and Qwen Code
---

# Agent Harness Portability

Canonical sources: `AGENTS.md`, `agents/*.md`, `agents/registry.json`, and `.agents/skills/*`. Tool files are adapters.

Symlink content only when the underlying format is identical. Generate adapters when schemas, permissions, tool names, or model controls differ.

Run:

```bash
python3 scripts/generate-agent-adapters.py
./scripts/setup-tool-links.sh
./scripts/verify-template.sh
```
