# Cargo workspace

[Cargo.toml](../../Cargo.toml) declares seven members:

| Member | Responsibility |
|---|---|
| `crates/domain-primitives` | Validated IDs and value types |
| `crates/generated-contracts` | REST/provider DTOs and signed payload contracts |
| `crates/bloque-rs` | Standalone unofficial Bloque HTTP client with examples; not a backend service dependency |
| `services/mock-provider` | Atomic simulated business runtime |
| `services/wallet` | Custody and durable signing jobs |
| `services/adapter-api` | Public REST/SSE, identity and delivery coordination |
| `apps/leptos-web` | Independent browser frontend |

Services own their storage models. Shared contracts depend on domain primitives;
the frontend consumes contracts without importing service implementations.
Backend images resolve the workspace but build only the three backend binaries,
without compiling browser assets.

The mock defaults to `storage-sqlite` and `mock-seed`. `storage-memory` is an
alternative; compiling both permits runtime selection through `MOCK_STORAGE`.
SQLite is the Compose default. Catalog seeding preserves existing edits.

The workspace uses edition 2024, resolver 3 and requires Rust 1.96.1. At this
revision `rust-toolchain.toml` selects moving `stable`, while Docker and release
CI explicitly select 1.96.1. The approved cleanup will align these declarations.
`Cargo.lock` pins dependency resolution. Release builds use thin LTO, one codegen
unit, stripped symbols and abort-on-panic. Workspace membership changes are
outside the current cleanup.
