#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc
cargo test -p mock-provider --no-default-features --features storage-memory,mock-seed --locked
cargo test -p mock-provider --no-default-features --features storage-sqlite,mock-seed --locked
# Feature-matrix integration tests also build binaries. Restore the combined
# backend before the separate end-to-end test runs both runtime storage choices.
cargo build --workspace --all-targets --all-features --locked
cargo check -p leptos-web --target wasm32-unknown-unknown --locked
cargo deny check
cargo audit
