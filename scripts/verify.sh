#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc
cargo build --workspace --all-targets --all-features --locked
cargo deny check
cargo audit
