#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python3 - "$ROOT/models/registry.json" <<'PY'
import json, sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text())
print(f"Verified on: {data['verified_on']}")
for alias, model in data['models'].items():
    print(f"{alias:10} {model['model_id']:24} {model['vendor']}")
PY
