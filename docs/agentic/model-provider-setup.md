# Model Provider Setup

Keep model choice separate from engineering rules. Roles, skills, specifications, and architecture must not change because a provider changes.

The verified model snapshot lives in `models/registry.json`.

## OpenCode

Connect the provider first:

```text
/connect
```

Then ask the template to resolve the current provider/model pair:

```bash
./scripts/select-opencode-model.sh deepseek
./scripts/select-opencode-model.sh glm
./scripts/select-opencode-model.sh kimi
./scripts/select-opencode-model.sh qwen
```

The script reads OpenCode's current model catalog. It does not guess a provider name.

## Qwen Code

Qwen Code can switch among configured providers with `/model`.

Generate the project provider catalog:

```bash
python3 scripts/configure-qwen-models.py
```

Choose `global` outside mainland China unless your Alibaba Cloud account requires the China endpoint.

The generated `.qwen/settings.json` stores model IDs, endpoint URLs, and environment-variable names. It does not store API keys.

Set only the keys you need:

```bash
export DEEPSEEK_API_KEY="..."
export ZAI_API_KEY="..."
export BAILIAN_CODING_PLAN_API_KEY="..."
```

Then run:

```bash
qwen
```

Use `/model` to select the active model.

Kimi K2.5 and Qwen 3.7 Plus use Alibaba Cloud Coding Plan in this template. DeepSeek V4 Flash and GLM-5.1 use their vendors' OpenAI-compatible endpoints.

## Codex and Claude Code

The project adapters for Codex and Claude Code inherit the harness model. Do not hardcode a third-party model into a role definition.

If a harness adds or changes a supported provider path, update only the provider layer. Keep canonical roles in `agents/` unchanged.

## Secrets

Never commit provider secrets. Prefer shell environment variables, an ignored local environment file, or a secret manager.

Do not copy API keys into agent prompts, specifications, issue bodies, logs, or CI output.
