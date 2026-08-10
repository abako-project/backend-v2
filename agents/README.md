# Agent Role Contracts

`agents/` is the canonical role layer.

A role answers **what am I responsible for?** A skill answers **how do I perform a reusable kind of work?**

Do not copy technical procedures into every role. Put them in `.agents/skills/` and reference them from `agents/registry.json`.

Generate tool adapters with:

```bash
python3 scripts/generate-agent-adapters.py
```
