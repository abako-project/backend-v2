#!/usr/bin/env bash
set -euo pipefail

command -v rustup >/dev/null || { echo "rustup is required." >&2; exit 1; }
command -v cargo >/dev/null || { echo "cargo is required." >&2; exit 1; }

rustup component add rustfmt clippy
rustup toolchain install nightly --profile minimal --component miri

install() {
  local name="$1"
  if cargo install --list | grep -q "^${name} v"; then
    echo "$name is already installed"
  else
    cargo install --locked "$name"
  fi
}

install cargo-nextest
install cargo-audit
install cargo-deny
install cargo-llvm-cov
install cargo-edit

echo "Development tools installed. Review versions and update policy before CI pinning."
