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


## Directory captured IDs (2026-10-22)

The signed bridge must reproduce version-1 IDs from the native provider's locked
`StdRng`, rather than substitute the SDK's rand 0.8 stream. Add **rand =0.10.2**
with defaults disabled and only `std_rng`; no thread RNG or system-entropy feature
is enabled. The shared shell already captures entropy once per operation.
An independent ID algorithm would diverge from accepted operations; supplying IDs
from a caller would fail to exercise the ordinary signed path.

The SDK lock adds exactly these four registry packages, with identical versions,
sources and checksums to the existing application lock; no existing package is
upgraded or removed:

| Package | Purpose / implications |
|---|---|
| rand 0.10.2 | Native version-1 `StdRng` stream; upstream safe API over ChaCha12 |
| rand_core 0.10.1 | RNG/seed traits; defaults disabled |
| chacha20 0.10.2 | Existing app's pure-Rust ChaCha12 implementation, RNG feature only; upstream architecture-specific unsafe/SIMD remains |
| cpufeatures 0.3.1 | Existing app's runtime CPU detection, with upstream platform/unsafe code and libc bindings |

All four declare MSRV 1.85 and MIT OR Apache-2.0. Published manifests/source and
current crates.io API metadata were checked on 2026-10-22: none is yanked. This
reuses the approved app dependency family; it adds no handwritten crypto/unsafe,
new native library or policy exception. `StdRng` is not a portable long-term
protocol; the exact lock and input version remain essential. A portable entropy
stream needs a separately reviewed version and later matching integration.

Primary metadata: [rand 0.10.2](https://crates.io/api/v1/crates/rand/0.10.2),
[rand_core 0.10.1](https://crates.io/api/v1/crates/rand_core/0.10.1),
[chacha20 0.10.2](https://crates.io/api/v1/crates/chacha20/0.10.2),
[cpufeatures 0.3.1](https://crates.io/api/v1/crates/cpufeatures/0.3.1).
The published `rand/src/rngs/std.rs` documents ChaCha12 and non-portability.
Independent xhigh M3 review accepted this exact delta and renewed fingerprint
at `9dccb98`, under the same four advisory IDs, thirteen versioned license
exceptions and December 1 expiry.
The application lock and all contract build locks remain unchanged.


M6 originally added a URL-validation helper. SPEC-0014 removes that contract,
its artifact and build lock. URL validation now remains in the adapter; contracts
store fixed hashes. No app/SDK dependency, lock or policy exception changes.
The SDK still uses its existing reviewed SHA-256 implementation for the narrow
legacy-reference conversion. Rebuilt canonical artifacts retain the same build
cohort and toolchain.


## M8 native-reference and HTTP link (exact-lock review approved)

The isolated candidate reuses the existing mock library through its verified-call
reference/read-model API and the unchanged generic HTTP router. Normal application
builds gain no SDK dependency. The mock's sr25519 signer was used only in tests
and is now a dev dependency; no mnemonic/key-derivation package enters this link.
The dependency delta adds15 packages, removes/upgrades none, and preserves every
existing registry checksum. Every added registry version is already in the app
lock. The unchanged four advisory/thirteen license exceptions and December1
expiry are not extended. Independent xhigh review at ddb4574 approved the exact fingerprint renewal.

Purpose/features: local mock-provider0.1.0 with storage-memory/mock-seed; axum0.8.9
with http1/json/query/tokio and no defaults; Tokio retains1.53.1 while adding
multithread runtime, net, signal and macros for the candidate process. Registry
metadata and published axum README were checked with cargo info --offline and
cargo metadata --locked on2026-10-22. Axum requires Rust1.80, axum-core1.78;
all added packages fit the candidate's1.96.1 baseline and existing license policy.
HTTP/runtime OS bindings retain their upstream unsafe/native implications already
reviewed in the app. No new crypto family is introduced. Alternative: a second
HTTP implementation or a new process protocol would duplicate authentication and
transport behavior; keeping the SDK isolated preserves the fast native build.

Added exact packages:

- axum 0.8.9
- axum-core 0.5.6
- errno 0.3.14
- matchit 0.8.4
- mime 0.3.17
- mock-provider 0.1.0
- ryu 1.0.23
- serde_path_to_error 0.1.20
- serde_urlencoded 0.7.1
- signal-hook-registry 1.4.8
- sync_wrapper 1.0.2
- tokio-macros 2.7.2
- tower 0.5.3
- tower-layer 0.3.3
- tower-service 0.3.3

Primary references: [axum0.8.9](https://docs.rs/axum/0.8.9/axum/),
[Tokio](https://docs.rs/tokio/1.53.1/tokio/) and exact registry manifests/checksums
in both lockfiles. The normal SDK policy scan remains required after fingerprint renewal;
additional findings are not silently waived.


## R2b1 advisory-fixing patch (2026-10-23)

A fresh policy scan found RUSTSEC-2026-0386 in the unchanged Tokio 1.53.1 lock.
Update the existing Tokio dependency to 1.53.3 in both workspaces, retaining every
feature. Both lock diffs change exactly one package version/checksum; complete
resolved package/feature comparisons confirm no other delta. No new dependency,
unsafe application code, native library, crypto family or advisory/license ignore.
The existing four advisory IDs, thirteen license exceptions and December 1 expiry
remain unchanged. The SDK lock fingerprint is now
`c03e9b652133062cbdf55dc8e5002cad3c245f5830dd6ece63ccb59c93b3099f`.

Primary published metadata checked: 1.53.3 is not yanked, MIT, Rust 1.71, checksum
`ce3335fa71841cda333a58d7615b03901380ecf09d59b3296d21f8bbac0dde4e`.
Published 1.53.1→1.53.3 manifests retain dependencies/features/license/MSRV; only
version and test declarations differ. Existing upstream OS/unsafe implications
remain. The patch adds explicit unsafe AsyncFd registration methods and deprecates
the old constructors; it does not erase their ownership requirements. Neither
project source nor another resolved package's src tree references AsyncFd.
Alternative: suppressing this new finding would expand the approved exceptions;
a compatible existing-dependency patch is narrower. Frozen predecessor binaries
remain offline development-transition fixtures, not deployment candidates.

References: [registry metadata](https://crates.io/api/v1/crates/tokio/1.53.3),
[upstream release](https://github.com/tokio-rs/tokio/releases/tag/tokio-1.53.3),
[upstream fix](https://github.com/tokio-rs/tokio/pull/8587),
[RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0386).
Astra xhigh preflight accepted this narrow scope; exact diff review and fresh
app/SDK/policy gates are recorded in the SPEC-0015 handoff. Root policy findings
remain unwaived.

## New unwaived SDK advisory (2026-10-24)

The preceding draft `cf1af9e` passed app CI but SDK CI stopped at dependency policy:
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html) now flags
unchanged `crypto-bigint 0.5.5` for variable-time operations intended to handle
secret integers in constant time. The primary advisory lists patched versions
`>=0.7.6`; exploitability depends on downstream usage, compiler/CPU and observation.
No project reachability/exploitability conclusion has been established.

The locked SDK path is `crypto-bigint 0.5.5 <- elliptic-curve 0.13.8 <-
ecdsa 0.16.9 / k256 0.13.4`. The published `elliptic-curve 0.13.8` manifest requires
`crypto-bigint = "0.5"`; upgrading the leaf to 0.7.6 is not a compatible lockfile
patch. The root application lock does not resolve crypto-bigint. Existing root
policy findings remain separate and unwaived.

No lock, exception, cryptographic implementation or dependency feature changed in
R3b2. App/SDK functional verification passing does not make this policy pass.
Prepare an upstream-compatible dependency remediation/review before promotion;
do not vendor cryptographic fixes or suppress the advisory as part of task-state
work. The existing exact-lock exceptions and expiry do not cover this advisory.

Primary evidence: [RustSec source](https://github.com/RustSec/advisory-db/blob/main/crates/crypto-bigint/RUSTSEC-2026-0387.md),
[upstream fix](https://github.com/RustCrypto/crypto-bigint/pull/1365), the exact
registry manifest pinned by the SDK lock, and the failed [SDK policy job](https://github.com/abako-project/backend-v2/actions/runs/40076423826/job/120820620541).
Local diagnostic log: main workspace `target/dev-artifacts/task-state-preceding-sdk-ci-failure.log`.

Fresh local confirmation on R3b2 source `6e8bf45`:
`bash scripts/verify.sh ink-sandbox-dependencies` returned exit 1: advisories
FAILED, bans/licenses/sources passed. Log: main workspace
`target/dev-artifacts/task-state-sdk-policy.log`. The failure is retained; no
policy fingerprint, lock or exception was changed.
