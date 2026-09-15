# Dependency Policy

Before adding a dependency, record its purpose, alternatives, required features, current metadata, MSRV, license and unsafe/native/cryptographic implications. Use [the baseline](../dependencies/poc-baseline.md) for current choices and AGENTS.md for approval boundaries.

Declare shared dependencies in Cargo.toml, minimize features and commit the resulting Cargo.lock. Use locked builds. The manifest's compatible version requirement and the exact resolved lockfile version are different facts; document which one is reported.

Use cargo deny and cargo audit for executable policy evidence. Current license and maintenance findings are tracked in [supply-chain security](../security/supply-chain.md). A permitted named maintenance exception must not disable unrelated vulnerability checks.

The project's existing Apache-2.0 package metadata does not settle every future FOSS distribution decision. Preserve third-party license notices and review the complete dependency graph before public distribution.

Dependency upgrades require relevant native/WASM builds, tests and updated audit evidence. Remove unused crates rather than maintaining speculative abstractions. Separately installed build tools and container packages need their own supply-chain review.
