# Dependency Version Snapshot — 2026-08-06

This is a date-stamped research snapshot, not an instruction to add every dependency. Re-query the registry and official documentation immediately before adoption, review features and MSRV, and commit the resulting lockfile. A newer release may exist after this date.

| Component | Version observed | Template decision |
|---|---:|---|
| Rust stable | 1.97.1 | Default toolchain snapshot; project may select stable after MSRV review |
| `serde` | 1.0.229 | Default serialization framework |
| `serde_json` | 1.0.151 | JSON boundaries when required |
| `thiserror` | 2.0.19 | Typed library and domain errors |
| `anyhow` | 1.0.104 | Executable composition boundaries only |
| `tokio` | 1.53.1 | Default async runtime |
| `axum` | 0.8.9 | Default HTTP framework |
| `sqlx` | 0.9.0 | Default asynchronous PostgreSQL access |
| `leptos` | 0.8.20 | Default all-Rust frontend candidate |
| `tonic` | 0.14.6 | Conditional internal gRPC |
| `async-graphql` | 7.2.1 | Default GraphQL BFF candidate |
| `lapin` | 4.10.0 | Default RabbitMQ client candidate |
| `tower` | 0.5.3 | Axum middleware foundation |
| `tower-http` | 0.7.0 | Common HTTP middleware |
| `rust_decimal` | 1.42.1 | Conditional exact decimal arithmetic |
| `futures` | 0.3.33 | Conditional stream and future combinators |
| `clap` | 4.6.6 | CLI parsing |
| `zbus` | 5.18.0 | Preferred D-Bus ecosystem for new work |
| `secrecy` | 0.10.3 | Conditional secret exposure and redaction |
| `zeroize` | 1.9.0 | Conditional best-effort memory erasure |
| `config` | 0.15.25 | Conditional layered configuration |
| `tokio-tungstenite` | 0.30.0 | Conditional WebSocket client/server plumbing |
| `argon2` | 0.5.3 | Password hashing candidate |
| `ed25519-dalek` | 3.0.0 | Ed25519 implementation candidate |
| `cargo-nextest` | 0.9.143 | Default test runner tool |
| `cargo-llvm-cov` | 0.8.7 | Coverage tool |
| `cucumber` | 0.23.0 | Optional executable Gherkin layer |

## Important Notes

- `miniserde` documentation explicitly describes it as a prototype and not a production-quality engineering artifact in the same sense as Serde. It remains an exceptional, measured choice rather than a default.
- `dotenv` is not the default recommendation. Use the maintained `dotenvy` fork for local development when `.env` loading is useful; production configuration should come from runtime configuration and secret management.
- `actix` and `actix-web` remain viable alternatives, but the base architecture does not mix them with Axum without an approved architectural reason.
- `crypto-common` is normally a transitive trait crate. Do not add it directly unless a concrete low-level cryptographic API requires it.
- The `ed25519` crate provides signature traits and formats; use an audited implementation crate such as `ed25519-dalek` for actual signing and verification.
- Cargo tools are developer and CI tools, not ordinary workspace runtime dependencies.

## Primary Source Locations

Use the current pages on the Rust release site, crates.io, docs.rs, and the official project documentation. The script `scripts/refresh-crate-catalog.sh` provides a quick registry refresh, but adoption still requires a dependency review.
