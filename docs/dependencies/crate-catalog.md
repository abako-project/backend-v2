# Rust Crate Catalog and Selection Policy

This retained template catalog lists candidates, not installed dependencies or
project approval. Cargo.toml/Cargo.lock and [poc-baseline.md](poc-baseline.md) describe
the current stack. PostgreSQL, RabbitMQ and GraphQL rows below are not current choices.
Verify registry and primary documentation before adopting a new dependency.

| Crate or tool | Default decision | Use | Important constraint |
|---|---|---|---|
| `serde` | Adopt | General typed serialization | Default serialization framework |
| `serde_json` | Adopt when JSON exists | JSON boundaries | Prefer typed DTOs over pervasive `Value` |
| `miniserde` | Exceptional | Narrow JSON-only, compile-size-sensitive cases | Requires measurement and documented acceptance of limited diagnostics and customization |
| `tokio` | Adopt | Async runtime | Bound work; no blocking on core threads |
| `axum` | Adopt | HTTP services and BFF | Use Tower middleware; keep handlers thin |
| `actix` | Alternative | Actor model | Do not add unless actor semantics are required |
| `actix-web` | Alternative | HTTP framework | Do not mix with Axum by default |
| `sqlx` | Adopt | Async PostgreSQL | Explicit features, transactions, migrations, disposable DB tests |
| `leptos` | Default frontend | Rust web UI | Confirm SSR/hydration and deployment requirements |
| `tonic`, `prost`, `tonic-build` | Conditional | Internal gRPC | Add only for justified RPC/streaming boundaries |
| `async-graphql` | Default GraphQL candidate | Browser-facing BFF | Authorization, batching, cost limits, schema diff |
| `lapin` | Default RabbitMQ client candidate | AMQP 0-9-1 | Confirms, manual acks, recovery, idempotency |
| `tokio-tungstenite` | Conditional | WebSocket protocol | Prefer Axum integration where sufficient |
| `futures` | Conditional | Stream/future combinators | Use standard/Tokio utilities when enough |
| `thiserror` | Adopt | Typed library errors | Preserve sources and stable variants |
| `anyhow` | Boundary-only | Executable startup/orchestration context | Do not expose as domain/public API contract |
| `clap` | Adopt for CLIs | Argument parsing | Separate parsing from execution; stable exit codes |
| `dotenv` | Do not adopt for new work | Legacy environment loading | Use maintained `dotenvy` for local development only |
| `dotenvy` | Conditional | Local `.env` loading | Production configuration comes from runtime environment/secret manager |
| `config` | Conditional | Layered configuration | Define precedence and validation explicitly |
| `rand` | Conditional | Randomness | Use CSPRNG-backed APIs for secrets; avoid modulo bias |
| `uuid` | Adopt when identifiers require it | Public/event/request IDs | Choose version semantics intentionally |
| `rust_decimal` | Conditional | Exact decimal calculations | Integer minor units remain simpler when adequate |
| `tracing`, `tracing-subscriber` | Adopt | Structured async diagnostics | Avoid holding entered-span guards across `.await` |
| `log` | Compatibility only | Libraries/ecosystem bridges | Applications use `tracing`; bridge legacy logs if needed |
| `crypto-common` | Usually transitive | Cryptographic trait plumbing | Do not add directly without a specific low-level need |
| `argon2` | Adopt for passwords | Password hashing | Parameters and migration policy require security approval |
| `ed25519` | Conditional trait | Signature abstraction | Pair with audited implementation such as `ed25519-dalek` |
| `secrecy` | Conditional | Explicit secret exposure/redaction | Not a substitute for key isolation or access control |
| `zeroize` | Conditional | Best-effort memory erasure | Document threat model and compiler/runtime limitations |
| `zbus` | Preferred for new D-Bus work | Linux D-Bus | Choose async runtime features deliberately |
| `dbus` | Legacy/compatibility | Existing bindings | Prefer `zbus` for new designs unless compatibility requires it |
| `bindgen` | Conditional | C/C++ FFI generation | Pin headers/toolchain and wrap unsafe API |
| `csv` | Conditional | CSV import/export | Define dialect, encoding, limits, validation, and injection handling |
| `tower`, `tower-http` | Adopt with Axum | Middleware and limits | Keep ordering and timeout semantics explicit |
| `reqwest` | Conditional | HTTP clients | Timeouts, TLS choice, retries, size limits, redaction |
| `time` or `chrono` | Conditional | Date and time | Select one primary model and define UTC/time-zone policy |
| `url` | Conditional | URL parsing | Do not hand-parse URLs |
| `validator` or domain constructors | Conditional | Input validation | Domain invariants still belong in domain types |
| `proptest` | Recommended | Property tests | Especially money, parsers, state machines, serialization |
| `testcontainers` | Recommended where useful | Disposable integration infrastructure | Pin images and make cleanup deterministic |
| `criterion` | Conditional | Microbenchmarks | Tie benchmarks to a stated performance requirement |
| `loom` | Conditional | Concurrency model checking | Use for custom synchronization, not general Tokio application tests |
| `cucumber` | Conditional | Executable Gherkin | Acceptance layer, not replacement for lower-level tests |
| `cargo-nextest` | Adopt tool | Test runner | Does not replace doc tests, Miri, or integration orchestration |
| `cargo-audit` | Adopt tool | RustSec advisories | Advisory policy and exceptions must be explicit |
| `cargo-deny` | Adopt tool | Licenses, sources, duplicate policy | Review policy per project |
| `cargo-llvm-cov` | Adopt tool | Coverage | Coverage is evidence, not a correctness target by itself |
| `cargo-edit` | Recommended tool | Controlled dependency editing | Review features and resulting lockfile |

## Selection Rule

No crate is added merely because it appears in this catalog. The implementing agent must show a concrete requirement and complete `dependency-review.md` for material additions.
