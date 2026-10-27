# native-provider

Native Rust implementation of marketplace rules, with memory or SQLite persistence.
It remains the default development provider while the isolated WASM provider's
promotion gates are open. Both implementations use the shared [provider](../../crates/provider/README.md)
execution and HTTP modules.

Start with [src/domain.rs](src/domain.rs), [src/storage.rs](src/storage.rs) and
[src/provider_api.rs](src/provider_api.rs). See [local setup](../../infra/README.md)
and [ownership boundaries](../../docs/architecture.md).
