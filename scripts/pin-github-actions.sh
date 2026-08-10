#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
command -v gh >/dev/null || { echo "gh is required" >&2; exit 1; }
gh auth status >/dev/null

resolve() {
  local repo="$1" ref="$2"
  gh api "repos/${repo}/git/ref/tags/${ref}" --jq '.object.sha' 2>/dev/null \
    || gh api "repos/${repo}/commits/${ref}" --jq '.sha'
}

checkout="$(resolve actions/checkout v4)"
toolchain="$(resolve dtolnay/rust-toolchain stable)"
cache="$(resolve Swatinem/rust-cache v2)"
mkdir -p .github/workflows
sed \
  -e "s|{{ACTIONS_CHECKOUT_SHA}}|$checkout|g" \
  -e "s|{{RUST_TOOLCHAIN_SHA}}|$toolchain|g" \
  -e "s|{{RUST_CACHE_SHA}}|$cache|g" \
  templates/github/ci.yml.tmpl > .github/workflows/ci.yml

echo "Rendered .github/workflows/ci.yml with full action commit SHAs. Review before commit."
