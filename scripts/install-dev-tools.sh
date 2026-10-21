#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
command -v rustup >/dev/null || { echo "rustup is required." >&2; exit 1; }

install_tool() {
  if ! cargo install --list | grep -q "^$1 v"; then
    cargo install --locked "$1"
  fi
}

case "${1:-app}" in
  app)
    rustup show active-toolchain
    rustup component add rustfmt clippy
    rustup target add wasm32-unknown-unknown
    echo "Rust application tools ready. PostgreSQL server tools and Python 3 are also required; see ci/README.md."
    ;;
  --security) install_tool cargo-deny; install_tool cargo-audit ;;
  --coverage) install_tool cargo-llvm-cov ;;
  --miri) rustup toolchain install nightly --profile minimal --component miri ;;
  *) echo "Usage: bash scripts/install-dev-tools.sh [app|--security|--coverage|--miri]" >&2; exit 2 ;;
esac
