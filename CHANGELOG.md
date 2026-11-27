# Changelog

## Unreleased

- Added a mode in which the API runs the contracts on an in-process fork of
  Kreivo, without the wallet and provider services. Set
  `KREIVO_URL`; see `services/api/README.md` and `infra/README.md`. Users are
  pallet-pass accounts, a password is the account's device and a passkey a
  restricted one, and the API keeps only session keys. The public API is the
  same. The default is unchanged.
- `contracts/openapi.json` lists the 409, 429, 500 and 503 answers the
  passkey routes can give. No request or response body changes.
- On a fork, `KREIVO_COMMUNITY` sets up a community with a usage subscription
  and admits every account to it, so that its pool pays their fees.
- Added the `chain-provider` crate and extended `kreivo` with fork reads,
  device replacement, passkeys and privileged fork calls.
- `scripts/poc-e2e.py --kreivo-snapshot <file>` runs the scenarios against the
  API on a fork.
- Renamed the product to Digesta, including backend image packages, browser
  configuration, session cookie and local development defaults. GHCR uses the
  repository owner's namespace. See `docs/project/digesta-rename.md` before
  reusing an existing local environment.
- Added `.github/workflows/release.yml`: `v*.*.*` tags on `master` run fmt,
  clippy, Cargo tests and doc tests, then publish `digesta-api`,
  `digesta-wallet` and `digesta-native-provider` to GHCR with semver tags and
  the Podman auto-update label.

## 2026.09.25

- Integrated `crates/bloque-rs`, the unofficial Bloque client, as a workspace
  member on workspace dependency versions and lints.
- Enabled HTTPS through `reqwest`'s `rustls` (aws-lc-rs) and allowed
  CDLA-Permissive-2.0 in `deny.toml`; see
  the [original dependency review](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies/bloque-client-2026-09-25.md).

## 2026.09.23

- Implemented versioned milestone delivery, current-submission rejection,
  public dispute opening, one counterparty response and project-wide freeze.
- Verified signed single- and four-milestone (5/3/4/2) happy paths and the
  dispute branch on SQLite and memory.
- Added the conceptual/API happy-path guide and reconciled project, contract,
  specification and delivery documentation.
- Removed unused browser API guidance and agent skill for the abandoned query
  protocol; the legacy-seeded catalog label remains domain data.

## 2026.09.15

- Updated POC documentation, native deployment commands and legacy coverage accounting.
- Verification scripts use native Cargo, Python and Docker commands.
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
