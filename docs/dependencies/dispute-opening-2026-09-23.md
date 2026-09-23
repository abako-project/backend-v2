# Dependency review: dispute evidence and tracing

Status: Accepted
Owner: DSP-05
Date: 2026-09-23

## Requirement

Validate untrusted evidence URLs with standard URL parsing and emit structured
service diagnostics for committed, rejected and unavailable provider operations.

## Decision

- `url 2.5.8`: direct dependency of generated-contracts. It parses absolute
  HTTPS URLs and exposes scheme, host and user-info checks. Hand-written URL
  parsing was rejected.
- `tracing 0.1.44`: direct dependency of adapter-api and mock-provider. It was
  already present transitively and is the workspace-standard diagnostics facade.
- `tracing-subscriber 0.3.23`: direct executable dependency with
  `default-features = false` and only `fmt`. It installs a stderr subscriber;
  filtering, JSON output and registry layers are not required by this PoC.

Official references: <https://docs.rs/url/2.5.8/url/>,
<https://docs.rs/tracing/0.1.44/tracing/> and
<https://docs.rs/tracing-subscriber/0.3.23/tracing_subscriber/>.

## Compatibility and licenses

The selected releases declare Rust 1.63 (`url`) and Rust 1.65
(`tracing`/`tracing-subscriber`) minimum supported versions, below this
workspace toolchain. `url` is MIT OR Apache-2.0; tracing crates are MIT.

These choices add no native system-library requirement, signing primitive,
secret handling or outbound network operation. URL parsing is structural only;
the services never fetch evidence. Transitive Rust dependencies remain governed
by the workspace lockfile.

## Removal criteria

Remove `url` only if the public contract stops accepting URL references.
Remove the subscriber only if service composition installs an equivalent
workspace-wide subscriber; keep the tracing facade at domain boundaries.
