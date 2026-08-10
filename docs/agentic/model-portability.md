# Model Portability

Agent roles are model-neutral. A model can change without changing a role, skill, specification, or architecture decision.

`models/registry.json` records model IDs verified on the date stored in the file. Treat it as a snapshot, not a permanent truth.

## Current Profiles

- Kimi: `kimi-k2.5`
- DeepSeek: `deepseek-v4-flash`
- GLM: `glm-5.1`
- Qwen: `qwen3.7-plus`

Run `./scripts/show-models.sh` to inspect the snapshot.

For OpenCode, connect a provider first and run `./scripts/select-opencode-model.sh <alias>`. The script asks OpenCode for its current model catalog and writes the exact discovered `provider/model-id` into `opencode.json`.

For Qwen Code, use `/auth` for Alibaba Cloud Coding Plan or run `python3 scripts/configure-qwen-models.py` to merge the verified provider catalog into `.qwen/settings.json`. Use `/model` to switch. Qwen Code reads `AGENTS.md`; project subagents live in `.qwen/agents/`, and project skills are exposed through `.qwen/skills/`.

Do not commit API keys. Do not hardcode an endpoint merely because another provider uses the same protocol shape.

## Updating a Model

1. Verify the new model ID in primary provider or harness documentation.
2. Update `models/registry.json` and its verification date.
3. Run representative project tasks with the old and new model.
4. Compare correctness, tool use, latency, and cost for your workload.
5. Change the default only when the evidence justifies it.

No role file should need editing during this process.
