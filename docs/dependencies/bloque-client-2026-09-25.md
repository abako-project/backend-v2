# Dependency review: Bloque client and TLS

Status: Accepted (human approval for native/cryptographic dependency, 2026-09-25)
Owner: Daniel Olano
Date: 2026-09-25

## Requirement

Kunveno talks to Bloque (on/off-ramp provider) through `crates/bloque-rs`, an
unofficial Rust client. Bloque's API is HTTPS-only, and the workspace `reqwest`
had no TLS backend.

## Decision

- `bloque` (`crates/bloque-rs`): workspace member, edition 2024, workspace lints
  and workspace dependency versions. Its wire shapes were reverse-engineered from
  `@bloque/sdk` 0.13.1, so treat them as unconfirmed until Bloque support says
  otherwise.
- `reqwest 0.13.4` with the `rustls` feature, enabled by `bloque`. This brings in
  `rustls 0.23.45`, `aws-lc-rs 1.18.0` (`aws-lc-sys 0.44.0`, native C code),
  `rustls-platform-verifier 0.7.1` and `webpki-root-certs 1.0.9`.
- `rand`, `uuid` (`v4`), `url`, `thiserror`, `serde`, `serde_json` and `tokio` use
  the existing workspace versions. The crate's own `reqwest 0.12`, `rand 0.8` and
  `rustls-tls` pins were dropped.

## Why aws-lc-rs and not ring

Cargo unifies features. `rustls-no-provider` on the shared `reqwest` makes every
`reqwest::Client` in the workspace panic unless a crypto provider was installed
first. That broke all `adapter-api` tests. The default `rustls` feature needs no
provider setup and keeps other crates unchanged.

## Compatibility and licenses

Declared MSRV: `rustls` 1.71, `aws-lc-rs` 1.71, `rustls-platform-verifier` 1.85;
all are below the workspace 1.96.1. `aws-lc-sys` compiles C, so builds need a C
compiler (container images included). Licenses: `aws-lc-rs` ISC AND
(Apache-2.0 OR ISC); `aws-lc-sys` also carries OpenSSL-derived components under
the permissive licenses listed in its metadata; `webpki-root-certs` is
CDLA-Permissive-2.0, which was added to `deny.toml` for this change.

TLS is compiled into every service through feature unification, but only
`bloque` is meant to call HTTPS endpoints.

## Operational notes

- Every call under `Session::swap()` moves real money. The crate does not gate
  them; the calling service must confirm intent and record the idempotency key.
- `Rate` carries quote values as `f64`. Treat them as display data. Money amounts
  are `String` on the wire; parse them into the project money type before any
  arithmetic.

## Removal criteria

Remove `bloque` if Bloque is dropped as a provider. Revisit the TLS provider if
the workspace decides to install one explicitly for all services.
