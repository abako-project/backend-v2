# identity service Agent Contract

## Responsibility

Defined by approved specifications for this bounded context.

## Ownership

This service owns its domain model, write schema, migrations, application behavior, and emitted event semantics.

## Forbidden Coupling

- Do not read or write another service's tables.
- Do not expose SQL row types as contracts.
- Do not change shared root files or public contracts without an assigned integration or contract task.

## Required Checks

```bash
cargo fmt --all -- --check
cargo check -p identity --all-targets --all-features
cargo clippy -p identity --all-targets --all-features -- -D warnings
cargo nextest run -p identity
```
