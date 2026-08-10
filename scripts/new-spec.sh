#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TITLE="${1:-}"
if [[ -z "$TITLE" ]]; then
  read -r -p "Feature title: " TITLE
fi
[[ -n "$TITLE" ]] || { echo "Feature title is required" >&2; exit 1; }

last="$(find specs -maxdepth 1 -type d -printf '%f\n' | grep -E '^[0-9]{4}-' | sort | tail -1 | cut -d- -f1 || true)"
if [[ -z "$last" ]]; then next=1; else next=$((10#$last + 1)); fi
id="$(printf '%04d' "$next")"
slug="$(printf '%s' "$TITLE" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')"
dir="specs/${id}-${slug}"
mkdir -p "$dir"
cp specs/_templates/spec.md "$dir/spec.md"
cp specs/_templates/acceptance.feature "$dir/acceptance.feature"
cp specs/_templates/design.md "$dir/design.md"
cp specs/_templates/data-model.md "$dir/data-model.md"
cp specs/_templates/threat-model.md "$dir/threat-model.md"
cp specs/_templates/tasks.md "$dir/tasks.md"
cp specs/_templates/status.md "$dir/status.md"
sed -i "s/SPEC-XXXX/SPEC-${id}/g; s/Feature Name/${TITLE}/g; s/Feature name/${TITLE}/g" "$dir"/*
echo "Created $dir"
