# Compatibility harness dependency review

Reviewed 2026-10-21 using published package manifests/source and `cargo info`.
The owner approved this native FRAME runtime family in SPEC-0013. These
packages remain isolated from application builds during milestone 0.

| Packages (exact direct pins) | Purpose / features |
|---|---|
| pallet-contracts 43.0.0 | Execute v5 WASM; std; no runtime benchmarks or try-runtime |
| frame-support/system 43.0.0 | Construct one in-process runtime; std |
| pallet-balances 44.0.0 | Technical execution/storage deposits; std |
| pallet-assets 46.1.0 | Real user asset transfers and conservation; std |
| pallet-timestamp 42.0.0 | Explicit fixture time; std |
| sp-core 38.1.0, sp-io 43.0.0, sp-runtime 44.0.0 | Account types, hashing, native externalities, raw trie persistence; std |
| parity-scale-codec 3.7.5, scale-info 2.11.6 | FRAME derives/ABI/checkpoint codec; derive |
| serde_json (locked) | Read ink artifact metadata; std |

FRAME/SDK packages are Apache-2.0. SCALE and serde packages are Apache-2.0 /
MIT. SDK manifests do not declare an MSRV: Rust 1.96.1 is the harness build
baseline, not an inferred upstream MSRV. The fixture build toolchain is recorded
separately in each artifact. Registry checksums and all transitives are locked.

The interpreter, trie implementation and SDK host interfaces bring unsafe and
native/cryptographic transitive code (including secp256k1 and hashing/signature
implementations). This proof writes no unsafe code, enables no experimental
cryptography and does not load these dependencies into the public API services.
Production integration needs a complete locked-graph advisory/license review;
existing application supply-chain findings are not waived by this review.

Alternatives considered: a full local parachain violates the approved developer
experience; generic WASM alone does not supply pallet-contracts storage/host
semantics; stock ink_sandbox 5.1.1 uses a different SDK cohort. A thin runtime
using the published pallet is the smallest compatible experiment. Only the
three needed Kreivo asset functions are implemented instead of importing its
unrelated listings/membership runtime dependencies and conflicting FRAME family.

Primary references:

- [pallet-contracts 43 metadata](https://crates.io/crates/pallet-contracts/43.0.0)
- [pallet-assets 46.1 metadata](https://crates.io/crates/pallet-assets/46.1.0)
- [pallet-timestamp 42 metadata](https://crates.io/crates/pallet-timestamp/42.0.0)
- [sp-io 43 metadata](https://crates.io/crates/sp-io/43.0.0)
- [Kreivo asset ABI and implementation at db0ccca](https://github.com/b-avb/kreivo/tree/db0ccca77dd0f8a437bfb07769f8afacb726d56a/apis/src)
- [ink_sandbox 5.1.1](https://docs.rs/ink_sandbox/5.1.1/ink_sandbox/)

## Observed policy result

`cargo deny --manifest-path tools/ink-sandbox/Cargo.toml check` fails the current
repository policy. Bans and sources pass; advisories and licenses fail. No
exception or allowlist change was added.

Maintenance advisories:

- libsecp256k1 0.7.2: RUSTSEC-2025-0161.
- parity-wasm 0.45.0: RUSTSEC-2022-0061.
- paste 1.0.15: RUSTSEC-2024-0436.
- proc-macro-error 1.0.4: RUSTSEC-2024-0370.

Rejected license expressions include GPL-3.0-only in Polkadot primitives and
XCM packages pulled by pallet-contracts; CC0-1.0 in Bitcoin/secp256k1/hash
packages; and Apache-2.0 WITH LLVM-exception in wasmparser-nostd. Deprecated SPDX
spellings also produce warnings. These are the scanner's policy findings, not
a legal conclusion about distribution.

The approved native runtime experiment does not waive dependency policy.
Milestone 2 provider integration needs an explicit disposition of this graph:
review acceptable upstream patches/replacements and obtain any required policy
approval. Do not change the SDK family just to silence the scan, relax the
allowlist implicitly, or ship the proof as an authoritative engine. The current
application dependency graph remains unchanged.

## Milestone 2 additions

Reviewed 2026-10-22 against the published registry manifests and primary source:

| Package | Purpose and features | MSRV / license |
|---|---|---|
| sp-state-machine =0.48.0 | Exhaustive fallible top/child trie iteration; std, already in this SDK lock | Not declared; Apache-2.0 |
| tokio =1.53.1 | Async replies and the worker's SQLite I/O runtime; rt, sync, time | 1.71; MIT |
| sqlx =0.9.0 | Atomic SQLite checkpoint plus opaque shell envelope; defaults off, runtime-tokio, sqlite-bundled | 1.94; MIT OR Apache-2.0 |

Tokio and SQLx use the same exact versions and SQLite feature family already
approved in the application; no new runtime family is introduced. SQLx's bundled
SQLite uses native C/FFI and a connection worker thread. Tokio uses upstream
unsafe/platform code. These implications already exist in the app, but remain
visible in the isolated workspace review. No new cryptographic algorithm or
unsafe Rust is written here. The standalone SDK graph is still not linked into
application services or images.

A std-only persistence file would avoid SQLx but could not prove the required
SQLite atomic commit with shell bytes. A second SQLite library would duplicate
native dependencies. A generic asynchronous executor would allow unmetered
closures; the worker instead accepts bounded typed call/instantiate requests.

Primary sources: [SQLx 0.9.0](https://crates.io/crates/sqlx/0.9.0),
[Tokio 1.53.1](https://docs.rs/tokio/1.53.1/tokio/sync/oneshot/index.html),
[sp-state-machine 0.48.0](https://crates.io/crates/sp-state-machine/0.48.0).

The owner requested **proposed** scoped exceptions on 2026-10-22. They are
[the policy record](policy/README.md). The later owner approval and integration
review below supersede the proposal's pending status.

## Shared execution shell (2026-10-22)

The owner approved the reviewed SDK exceptions for integration work. The active
SDK policy uses exactly those four advisory IDs and thirteen package/version
license exceptions; no root policy waiver was added. The renewed lock fingerprint
includes the shared shell and wire crates. Registry additions to the SDK graph
already exist in the reviewed app graph; no registry package version or SDK
cohort changed. Application root Cargo.lock adds only the local execution crate.

`provider-execution` shares the existing envelope logic between the working Rust
provider and the candidate worker. It uses schnorrkel **=0.11.5**, defaults off,
std only, for verification. That exact primitive is already locked and used by
both subxt-signer and the SDK. Published metadata: BSD-3-Clause, no declared
MSRV; tested here on Rust 1.96.1. The std/curve/hash platform code retains the
existing transitive unsafe/cryptographic implications; no new native library or
cryptographic family is introduced. Verification parses the same public key and
signature bytes and uses the same `substrate` context as subxt-signer 0.50.3's
published verifier. App tests sign with Subxt; SDK tests sign with sp-core.

The alternative of importing the full signer into the runtime brought unused
mnemonic/key-derivation APIs and an additional unapproved bip39 license exception.
The verification-only primitive avoids that expansion while retaining the same
signature semantics. Wallet key handling and signing remain unchanged.

Primary references: [schnorrkel 0.11.5](https://crates.io/crates/schnorrkel/0.11.5),
[subxt sr25519 source](https://docs.rs/subxt-signer/0.50.3/src/subxt_signer/sr25519.rs.html).
The published Cargo.toml/source were also inspected locally before this change.
