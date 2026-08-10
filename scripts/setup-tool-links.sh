#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
force=false
[[ "${1:-}" == "--force" ]] && force=true

link_dir() {
  local link="$1" target="$2"
  mkdir -p "$(dirname "$link")"

  if [[ -L "$link" ]]; then
    local current
    current="$(readlink "$link")"
    if [[ "$current" == "$target" ]]; then
      echo "OK: $link -> $target"
      return
    fi
    rm "$link"
  elif [[ -e "$link" ]]; then
    if [[ "$force" != true ]]; then
      echo "Refusing to replace real path: $link" >&2
      echo "Move it yourself or rerun with --force after reviewing its contents." >&2
      exit 1
    fi
    rm -rf "$link"
  fi

  ln -s "$target" "$link"
  echo "Linked: $link -> $target"
}

link_dir .claude/skills ../.agents/skills
link_dir .qwen/skills ../.agents/skills
