# Supply-Chain Security

The workspace commits Cargo.lock and uses locked builds. Dependency selection records are in [poc-baseline.md](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies/poc-baseline.md); detailed package findings are in [the dependency audit](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies/poc-audit-2026-09-10.md).

```sh
cargo deny check
cargo audit
```

## Current dependency findings

The November 12, 2026 refresh updates the affected TLS/certificate packages,
`ctutils` and bundled SQLite. The application graph no longer reports the eight
vulnerability advisories found before the update. No advisory ignore was added.

SQLx is pinned to upstream revision `10462fa` until a release supports patched
`libsqlite3-sys 0.39.1`. Both workspaces use the same revision. This includes other
unreleased SQLx fixes, so verification covers PostgreSQL transactions as well as
wallet/provider SQLite persistence. The Git source allowance is limited to SQLx
and requires a revision; the provider policy also pins its entire lockfile.

The root policy still rejects the existing CC0-1.0 and Boost BSL-1.0 licenses in
`base16`, `bip39`, `bitcoin_hashes`, `hex-conservative` and `xxhash-rust`. It also
rejects the unmaintained `paste` and `proc-macro-error2` packages. These were not
silenced by the update. The separate provider policy retains its previously
approved exceptions and December 1 expiry; see its [scope](../../services/contract-provider/policy/README.md).

Dependabot watches both Cargo workspaces. Provider updates need review of the
exact-lock policy before they can pass CI. Application checks and dependency-policy
checks remain separate; see [verification](../../ci/README.md). A passing
`cargo-deny` advisory check is not a claim that every dependency is safe.

Docker frontend builds use wasm-bindgen-cli directly; the earlier Trunk build-tool graph was removed. Application-lock auditing does not by itself audit separately installed build tools or container packages.

Unsafe/native/cryptographic dependencies require the review defined in AGENTS.md. Production image scans, published SBOMs, signed provenance and artifact signing are not implemented release guarantees.

## Project metadata decision

The root package metadata declares `Apache-2.0`, while
[LICENSE-TEMPLATE-NOTICE.md](../../LICENSE-TEMPLATE-NOTICE.md) describes a placeholder
`Apache-2.0 OR MIT` choice and asks for the actual license texts. The cleanup
preserves both pending an explicit owner decision. It does not select a license
or resolve dependency license-policy findings.
