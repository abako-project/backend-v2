#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
alias_name="${1:-}"

if [[ -z "$alias_name" ]]; then
  echo "Usage: $0 <kimi|deepseek|glm|qwen>" >&2
  exit 2
fi

command -v opencode >/dev/null || { echo "OpenCode is not installed or not on PATH." >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required." >&2; exit 1; }

model_id="$(python3 - "$alias_name" <<'PY'
import json, sys
from pathlib import Path
alias = sys.argv[1]
data = json.loads(Path('models/registry.json').read_text())
try:
    print(data['models'][alias]['model_id'])
except KeyError:
    raise SystemExit(f"Unknown model alias: {alias}")
PY
)"

mapfile -t candidates < <(opencode models --refresh 2>/dev/null | awk -v id="$model_id" '$0 == id || $0 ~ ("/" id "$") {print $0}')

if (( ${#candidates[@]} == 0 )); then
  echo "No connected OpenCode provider exposes '$model_id'." >&2
  echo "Open OpenCode, run /connect, connect the intended provider, then retry." >&2
  exit 1
fi

if (( ${#candidates[@]} == 1 )); then
  selected="${candidates[0]}"
else
  echo "More than one provider exposes $model_id:"
  for i in "${!candidates[@]}"; do
    printf '  %d) %s\n' "$((i + 1))" "${candidates[$i]}"
  done
  read -r -p "Choose provider [1]: " choice
  choice="${choice:-1}"
  [[ "$choice" =~ ^[0-9]+$ ]] || { echo "Invalid choice." >&2; exit 2; }
  index=$((choice - 1))
  (( index >= 0 && index < ${#candidates[@]} )) || { echo "Choice out of range." >&2; exit 2; }
  selected="${candidates[$index]}"
fi

python3 - "$selected" <<'PY'
import json, sys
from pathlib import Path
selected = sys.argv[1]
p = Path('opencode.json')
data = json.loads(p.read_text())
data['model'] = selected
p.write_text(json.dumps(data, indent=2) + '\n')
print(f"OpenCode default model set to: {selected}")
PY
