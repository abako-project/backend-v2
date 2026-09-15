# Supply-Chain Security

The workspace commits Cargo.lock and uses locked builds. Dependency selection records are in [poc-baseline.md](../dependencies/poc-baseline.md); detailed package findings are in [the dependency audit](../dependencies/poc-audit-2026-09-10.md).

```sh
cargo deny check
cargo audit
```

The last recorded policy check failed on CC0-1.0/Boost BSL-1.0 license handling and the two Leptos transitive maintenance advisories, while bans and sources passed. The user accepted deferring the named maintenance warnings for this POC; that decision does not change deny.toml by itself. Do not report a passing gate until configuration and executable evidence agree. Maintenance advisories and license-policy failures are different from known vulnerabilities.

Docker frontend builds use wasm-bindgen-cli directly; the earlier Trunk build-tool graph was removed. Application-lock auditing does not by itself audit separately installed build tools or container packages.

Unsafe/native/cryptographic dependencies require the review defined in AGENTS.md. Production image scans, published SBOMs, signed provenance and artifact signing are not implemented release guarantees.
