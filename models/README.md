# Model Profiles

This directory keeps model selection out of agent roles. `registry.json` is a dated snapshot of verified model IDs.

```bash
./scripts/show-models.sh
./scripts/select-opencode-model.sh deepseek
./scripts/select-opencode-model.sh glm
./scripts/select-opencode-model.sh kimi
./scripts/select-opencode-model.sh qwen
```

The OpenCode selector resolves the provider prefix from the live model catalog. It does not guess a provider name.
