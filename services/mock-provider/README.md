# mock-provider

Native Rust implementation of marketplace rules, with memory or SQLite persistence.
It remains the default development provider while the isolated WASM provider's
promotion gates are open. Its private HTTP interface is also used by the
[contract provider](../../tools/ink-sandbox/README.md); that runtime executes WASM rules.

Start with [src/domain.rs](src/domain.rs), [src/storage.rs](src/storage.rs) and
[src/provider_api.rs](src/provider_api.rs). See [local setup](../../infra/README.md)
and [ownership boundaries](../../docs/architecture.md).
