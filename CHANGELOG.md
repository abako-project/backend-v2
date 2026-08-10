# Changelog

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
