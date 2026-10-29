# Dependencies

The root [manifest](../Cargo.toml) and [lockfile](../Cargo.lock) define the
application graph. The embedded runtime has its own [manifest](../services/contract-provider/Cargo.toml),
lockfile and [policy](../services/contract-provider/policy/README.md), so its SDK does not
enter normal application builds. Use locked builds and minimal features.

| Component | Selected tools and reason |
|---|---|
| HTTP and async | Axum and Tokio; bounded asynchronous service work |
| Persistence | SQLx with PostgreSQL for API data and SQLite for wallet/provider state |
| Wire data | Serde/JSON for public transport; SCALE for signed provider calls |
| Signing | Existing sr25519/Schnorrkel implementation, with keys confined to wallet |
| Evidence | Standard `url` parsing for structural HTTPS validation; no URL fetching and no parser in contracts |
| Diagnostics | `tracing` and `tracing-subscriber` with formatting only |
| Contract execution | ink! 5.1.1 artifacts in the isolated FRAME/pallet-contracts runtime |
| Bloque | Standalone unofficial client; not a backend service integration |

The Bloque client's accepted TLS feature uses reqwest/rustls and aws-lc-rs,
including native C code. Cargo feature unification also compiles TLS into workspace
services. Switching to a no-provider TLS feature requires explicit initialization;
it previously broke clients. Bloque quote floats are display data only; application
money remains checked integer units. The client's wire shapes remain unconfirmed
against the external provider, and its transfer methods are not a simulation.

Security and license findings are recorded in [supply-chain status](security/supply-chain.md)
and the SDK policy. Existing maintenance/license exceptions do not waive new
vulnerability findings. Build tools and container packages need separate review.
Package metadata does not settle the repository's outstanding license decision.

Before adding or upgrading a dependency, record its purpose, alternatives,
features, exact source/version, maintenance, MSRV, license and unsafe/native/crypto
implications in the PR. Use primary package metadata. Follow the approval boundaries
in [Contributing](../CONTRIBUTING.md) and run applicable native/WASM and policy checks.

Historical selection evidence remains in [Git](https://github.com/abako-project/backend-v2/tree/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies),
including the accepted URL/tracing and Bloque TLS reviews. Current pinned versions
come from lockfiles, not old version inventories.
