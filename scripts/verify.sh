#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  echo "Usage: bash scripts/verify.sh [app|ink-sandbox|ink-sandbox-dependencies|containers|dependencies]"
}
[[ $# -le 1 ]] || { usage >&2; exit 2; }
case "${1:-app}" in
  app)
    for tool in cargo python3 initdb pg_ctl psql; do
      command -v "$tool" >/dev/null || { echo "Required command missing: $tool (see ci/README.md)" >&2; exit 1; }
    done
    cargo fmt --all -- --check
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    python3 scripts/test_postgres.py cargo test --workspace --all-targets --all-features --locked
    cargo test --workspace --all-features --doc --locked
    cargo test -p mock-provider --no-default-features --features storage-memory,mock-seed --locked
    cargo test -p mock-provider --no-default-features --features storage-sqlite,mock-seed --locked
    # Restore both compiled storage choices after the feature-matrix builds.
    cargo build --workspace --all-targets --all-features --locked
    cargo check -p leptos-web --target wasm32-unknown-unknown --locked
    target_dir="$(cargo metadata --no-deps --format-version 1 --locked | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
    python3 scripts/poc-e2e.py --binaries "$target_dir/debug"
    ;;
  ink-sandbox)
    manifest=tools/ink-sandbox/Cargo.toml
    cargo fmt --manifest-path "$manifest" -- --check
    cargo check --manifest-path "$manifest" --all-targets --all-features --locked
    cargo clippy --manifest-path "$manifest" --all-targets --all-features --locked -- -D warnings
    cargo test --manifest-path "$manifest" --all-features --locked
    cargo build --manifest-path "$manifest" --all-targets --all-features --locked
    ;;
  ink-sandbox-dependencies)
    python3 tools/ink-sandbox/policy/check.py
    ;;
  containers)
    # Rendering uses synthetic values; no application secret is read.
    python3 infra/verify.py
    for service in adapter-api wallet mock-provider; do
      docker build -f infra/backend.Dockerfile --target "$service" -t "digesta-$service:verification" .
    done
    ;;
  dependencies)
    # Run both tools, preserving a failing result from either.
    result=0
    cargo deny check || result=1
    cargo audit || result=1
    exit "$result"
    ;;
  -h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
