# Local POC dependency baseline

Reviewed 2026-09-08 and updated 2026-09-14 using Context7, primary docs, and Cargo metadata. This is a verified baseline rather than a claim that every package is the latest patch. Resolve and audit Cargo.lock before handoff.

| Family | Baseline and purpose | Features / implications |
|---|---|---|
| HTTP/runtime | Axum 0.8.9, Tokio 1.53.1, tower-http 0.6.11 | HTTP1, JSON, queries; runtime, bounded synchronization, CORS; no GraphQL |
| SQLite | SQLx 0.9.0 | runtime-tokio + sqlite-bundled; Rust >=1.94, native SQLite C/FFI, no extension loading |
| Wire types | serde 1.0.229, serde_json 1.0.151, SCALE 3.7.5 | derive, checked boundary types; JSON for HTTP and SCALE for signed mock calls |
| Errors | thiserror 2.0.20 | typed library/application errors |
| Internal client | reqwest 0.13.4 | defaults disabled, json/query; private HTTP only, do not use for external sensitive traffic without reviewed TLS |
| Events | tokio-stream 0.1.18 | sync/time only as required; durable cursor remains in owning database |
| IDs/time | uuid 1.24.1, chrono 0.4.45 | std; chrono ISO weeks without host timezone or tzdata |
| Entropy/sampling | getrandom 0.4.3, rand 0.10.2 | fallible OS entropy for secrets; random sampling only for mock score ties |
| Signing | subxt-signer 0.50.1 | defaults disabled, sr25519/std; no full Subxt client, ECDSA, or libsecp256k1 |
| Seed encryption | chacha20poly1305 0.11.0 | alloc + explicit zeroize; XChaCha20-Poly1305 key32/nonce24; nonce generated externally |
| Secret handling | zeroize 1.9.0, argon2 0.5.3, sha2 0.10.9 | Argon2id password hashes outside Tokio core threads; 0.10 SHA2 aligns with signer dependency |
| Browser | Leptos 0.8.20, gloo-net 0.7.0, wasm-bindgen-cli 0.2.128 | CSR/http/json; direct static WebAssembly build, no SSR/server functions |

All baseline MSRVs fit Rust 1.96.1. Leptos requires 1.88, SQLx 1.94, signer/AEAD 1.85; older mainstream dependencies require less. Exact resolved licenses and transitives must pass cargo-deny/audit. Main dependencies permit MIT and/or Apache-2.0; Subxt's GPL option is an alternative, not a requirement. SQLite introduces native/unsafe implementation dependencies while project code forbids unsafe.

Alternatives intentionally omitted: broker for durable polling, GraphQL, full blockchain client for mock signing, rusqlite alongside SQLx (incompatible native SQLite dependency versions), server-rendered frontend, Trunk in the container build, and automatic proxy discovery. Trunk 0.21.14's locked installer was removed after its build-only graph was found to contain vulnerable crossbeam-channel 0.5.14; the application dependency graph never contained that crate.

Primary sources: [Subxt signer](https://github.com/paritytech/subxt/blob/master/signer/src/sr25519.rs), [SQLx SQLite](https://docs.rs/sqlx/latest/sqlx/sqlite/index.html), [AEAD](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/), [reqwest features](https://docs.rs/reqwest/latest/reqwest/), [Leptos CSR deployment](https://github.com/leptos-rs/book/blob/main/src/deployment/csr.md), [Chrono ISO dates](https://docs.rs/chrono/latest/chrono/naive/struct.NaiveDate.html).
