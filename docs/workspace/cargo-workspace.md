# Cargo Workspace Architecture

Cargo.toml contains six members:

| Member | Responsibility |
|---|---|
| crates/domain-primitives | Stable validated IDs and value types |
| crates/generated-contracts | REST/provider DTOs and signed payload contracts |
| services/mock-provider | Atomic simulated business runtime |
| services/wallet | Custody and durable signing jobs |
| services/adapter-api | Public REST/SSE, identity and delivery coordination |
| apps/leptos-web | Independent browser frontend |

Only these six packages belong to the workspace. The unused service, worker and library scaffolds were removed; add a package only when an approved task needs it.

Service implementations own their storage models. Shared contracts depend on domain primitives; the frontend consumes contracts without importing backend service code. Backend images build independently of browser tooling.

The mock's default features are storage-sqlite and mock-seed. storage-memory is an alternative; an all-features build permits runtime selection. SQLite is the Compose default. Catalogue seeding preserves existing edits and the fixed coordinator role rule.

The workspace uses edition 2024, resolver 3 and the toolchain declared in rust-toolchain.toml. Cargo.lock pins resolved versions. Release settings include thin LTO, one codegen unit, stripped symbols and abort-on-panic. Keep feature combinations covered by verification before changing membership or dependency direction.
