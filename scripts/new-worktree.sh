#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ISSUE="${1:?Usage: $0 <issue> <role> <slug> [base-branch]}"
ROLE="${2:?Usage: $0 <issue> <role> <slug> [base-branch]}"
SLUG="${3:?Usage: $0 <issue> <role> <slug> [base-branch]}"
BASE="${4:-main}"
BRANCH="agent/${ROLE}/${ISSUE}-${SLUG}"
WORKTREE="../worktrees/${ISSUE}-${ROLE}-${SLUG}"

git rev-parse --is-inside-work-tree >/dev/null
git worktree add -b "$BRANCH" "$WORKTREE" "$BASE"
printf 'Branch: %s\nWorktree: %s\n' "$BRANCH" "$WORKTREE"
