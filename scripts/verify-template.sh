#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

required=(
  AGENTS.md CLAUDE.md README.md SPECS.md
  agents/registry.json models/registry.json
  .codex/config.toml opencode.json
  specs/_templates/spec.md specs/_templates/acceptance.feature
  docs/agentic/writing-standard.md docs/agentic/portability.md docs/agentic/model-portability.md
  docs/agentic/model-provider-setup.md templates/qwen/model-providers.json.tmpl
  docs/dependencies/crate-catalog.md
  scripts/configure-project.sh scripts/init-workspace.sh
  scripts/generate-agent-adapters.py scripts/setup-tool-links.sh scripts/configure-qwen-models.py
)
for path in "${required[@]}"; do
  [[ -e "$path" ]] || { echo "Missing $path" >&2; exit 1; }
done

[[ -L .claude/skills ]] || { echo ".claude/skills must be a symlink" >&2; exit 1; }
[[ "$(readlink .claude/skills)" == "../.agents/skills" ]] || { echo "Unexpected .claude/skills target" >&2; exit 1; }
[[ -L .qwen/skills ]] || { echo ".qwen/skills must be a symlink" >&2; exit 1; }
[[ "$(readlink .qwen/skills)" == "../.agents/skills" ]] || { echo "Unexpected .qwen/skills target" >&2; exit 1; }

while IFS= read -r skill_file; do
  grep -q '^name: ' "$skill_file" || { echo "Missing skill name: $skill_file" >&2; exit 1; }
  grep -q '^description: ' "$skill_file" || { echo "Missing skill description: $skill_file" >&2; exit 1; }
done < <(find .agents/skills -name SKILL.md -type f | sort)

python3 - <<'PY'
import json
import tomllib
from pathlib import Path

for path in ['opencode.json', 'agents/registry.json', 'models/registry.json']:
    json.loads(Path(path).read_text())
for path in ['.codex/config.toml', 'deny.toml', '.config/nextest.toml', '.cargo/config.toml']:
    tomllib.loads(Path(path).read_text())
for path in Path('.codex/agents').glob('*.toml'):
    tomllib.loads(path.read_text())

models = json.loads(Path('models/registry.json').read_text())
expected_models = {
    'kimi': 'kimi-k2.5',
    'deepseek': 'deepseek-v4-flash',
    'glm': 'glm-5.1',
    'qwen': 'qwen3.7-plus',
}
actual_models = {name: profile['model_id'] for name, profile in models['models'].items()}
if actual_models != expected_models:
    raise SystemExit(f'Unexpected verified model snapshot: {actual_models}')

registry = json.loads(Path('agents/registry.json').read_text())
expected = {a['id'] for a in registry['agents']}
for directory, suffix in [
    ('.codex/agents', '.toml'),
    ('.opencode/agents', '.md'),
    ('.claude/agents', '.md'),
    ('.qwen/agents', '.md'),
]:
    found = {p.stem for p in Path(directory).glob(f'*{suffix}')}
    if found != expected:
        raise SystemExit(f'Adapter set mismatch in {directory}')
print('Structured files and adapter sets validated.')
PY

for script in scripts/*.sh; do bash -n "$script"; done
python3 -m py_compile scripts/generate-agent-adapters.py scripts/configure-qwen-models.py

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
for d in .codex/agents .opencode/agents .claude/agents .qwen/agents; do
  mkdir -p "$tmp/$d"
  cp -a "$d/." "$tmp/$d/"
done
python3 scripts/generate-agent-adapters.py >/dev/null
for d in .codex/agents .opencode/agents .claude/agents .qwen/agents; do
  diff -ru "$tmp/$d" "$d" >/dev/null || { echo "Generated adapter drift in $d" >&2; exit 1; }
done

echo "Template validation passed."
