# Changelog

## 2026.09.15

- Updated POC documentation, native deployment commands and legacy coverage accounting.
- Removed RTK as a dependency of user-facing verification scripts.
- Kept independent verification and key-rotation scope explicitly open.

## 2026.09.14

- Integrated Rust adapter, encrypted custody, atomic mock and independent Leptos.
- Added shared contracts, negotiated planning, escrow, reputation and signed E2E.
- Verified Compose/Nginx and switched the frontend container to wasm-bindgen-cli.

August entries below describe the original template, not the current application.

## 2026.08.10

- Added a canonical agent registry and generated adapters for Codex, OpenCode, Claude Code, and Qwen Code.
- Added full-directory skill symlinks for Claude Code and Qwen Code.
- Kept `.agents/skills/` as the only skill source of truth.
- Added model-neutral profiles for Kimi K2.5, DeepSeek V4 Flash, GLM-5.1, and Qwen 3.7 Plus.
- Added an OpenCode model selector that resolves the current provider prefix from the live catalog.
- Added the writing standard: simplicity, brevity, clarity, and humanity.
- Added documentation, performance, model-selection, and harness-portability skills.
- Kept the template Cargo-first with no required `justfile` or active project `Cargo.toml`.

## 2026.08.06

- Initial production-oriented agentic Rust template.
