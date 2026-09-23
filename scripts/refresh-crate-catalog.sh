#!/usr/bin/env bash
set -euo pipefail
crates=(serde serde_json miniserde rust_decimal tokio axum thiserror anyhow sqlx leptos clap dotenvy rand uuid tracing crypto-common argon2 ed25519 ed25519-dalek futures actix actix-web zbus tokio-tungstenite csv tonic lapin tower tower-http secrecy zeroize)
for crate in "${crates[@]}"; do
  cargo search "$crate" --limit 1
  sleep 0.2
done
