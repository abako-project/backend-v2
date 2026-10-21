# SDK policy proposal

Status: **PROPOSED, awaiting owner review.** Requested 2026-10-22.
No normal verification command, CI job or provider loads this configuration.
The root `deny.toml` is unchanged. This proposal does not approve distribution,
provider integration, images or deployment.

The proposed configuration keeps the root allowlist, source restrictions,
yanked-package check and bans. It adds only the entries below. It is limited to
this standalone candidate workspace. License exceptions select exact package
versions; advisory ignores select exact advisory IDs. The installed cargo-deny
0.20.2 cannot constrain an advisory ignore by package version/parent (newer online
docs describe unsupported keys), so the evaluation script pins the entire lockfile
by SHA-256 and refuses changes. It also expires on 2026-12-01. Both restrictions
belong to the wrapper; invoking cargo-deny directly bypasses them.

```sh
python3 tools/ink-sandbox/policy/check_proposal.py
```

This fetches current advisory data, so the same proposal can still fail on a new
advisory. No vulnerability, unsoundness or yanked-package finding is broadly
suppressed. Evaluate this graph again before adopting a provider integration.

## Maintenance advisories

All four are unmaintained/deprecated notices, with no patched version reported
by RustSec. Acceptance retains maintenance risk; it does not demonstrate that the
code is safe. Proposed acceptance lasts only through the review date.

| Package / advisory | Exposure and reason | Replacement path |
|---|---|---|
| libsecp256k1 0.7.2 / RUSTSEC-2025-0161 | SDK crypto host implementation, retained by sp-core/sp-io; contracts could call crypto hosts. Wallet signing remains separate. | SDK cohort using maintained k256; rerun ABI/asset/restart proof. |
| parity-wasm 0.45.0 / RUSTSEC-2022-0061 | WASM parsing/instrumentation through sp-version and wasm-instrument. Worker accepts only verified pinned artifacts, reducing arbitrary upload exposure. | Upstream wasm-tools port/coherent SDK update; verify metering/reverts. |
| paste 1.0.15 / RUSTSEC-2024-0436 | Compile-time macro expansion across FRAME, arithmetic and interpreter dependencies. Pinned build inputs reduce drift. | Upstream pastey/direct macro replacements; rebuild and review output. |
| proc-macro-error 1.0.4 / RUSTSEC-2024-0370 | Build-time aquamarine macro dependency. | Upstream proc-macro-error2 migration; verify the SDK build. |

Primary notices: [libsecp256k1](https://rustsec.org/advisories/RUSTSEC-2025-0161.html),
[parity-wasm](https://rustsec.org/advisories/RUSTSEC-2022-0061.html),
[paste](https://rustsec.org/advisories/RUSTSEC-2024-0436.html),
[proc-macro-error](https://rustsec.org/advisories/RUSTSEC-2024-0370.html).
A patched fork or SDK update is a separate reviewed change, not an implicit
substitution to make the scanner green.

## License expressions

These are package-specific proposed policy permissions, not a conclusion about
license compatibility or fulfillment of distribution obligations. In particular,
allowing GPL-3.0-only in a scanner does not resolve obligations for a distributed
combined executable. Owner review must address that before application linkage
or distribution. Do not relabel upstream licenses or add GPL/CC0 globally.

| Expression | Exact packages |
|---|---|
| GPL-3.0-only | polkadot-core-primitives 20.0.0; polkadot-parachain-primitives 19.0.0; staging-xcm 19.0.0; staging-xcm-builder 23.0.0; staging-xcm-executor 22.0.0; xcm-procedural 11.0.2 |
| CC0-1.0 | bitcoin_hashes 0.13.1; hex-conservative 0.1.2; parity-bip39 2.0.1; secp256k1 0.28.2; secp256k1-sys 0.9.2; tiny-keccak 2.0.2 |
| Apache-2.0 WITH LLVM-exception | wasmparser-nostd 0.100.2 |

These arrive through pallet-contracts' SDK/XCM, crypto and WASM dependency paths.
Removing unused public XCM APIs from our runtime does not remove the upstream
crate dependencies. A coherent smaller SDK graph is an alternative if the
expressions are unacceptable; its compatibility proof must be repeated.
Published manifests and license files are available under each exact package
version on crates.io. [cargo-deny license exceptions](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html#exceptions)
explain why package-level permissions are narrower than a global allowlist.

## Observed evaluation

2026-10-22, cargo-deny 0.20.2, locked graph: proposed configuration passes
advisories, bans, licenses and sources. Multiple-version/deprecated-SPDX warnings
remain. The normal policy still rejects the recorded SDK maintenance/license
findings. A successful proposal scan is evidence for review, not active policy
approval or a completed provider-integration gate.
