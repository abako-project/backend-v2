# ink! v5 compatibility proof

This is milestone 0 of [SPEC-0013](../../specs/0013-ink-v5-sandbox/spec.md).
It runs production WASM in a native FRAME runtime without starting a node. It
is deliberately a separate Cargo workspace and lockfile: the application keeps
its current dependencies, authoritative Rust engine and startup behavior.

```sh
bash scripts/verify.sh ink-sandbox
```

The proof executes Calendar → Ratings and Project → Calendar calls, direct
caller asset deposits, contract balance queries and payouts using real
`pallet-assets`. It checks a failed deposit after tentative task mutations,
revert flags, finite gas, denied origins, malformed selectors and restoration
of contract Mapping data from a serialized raw-trie checkpoint. Contracts use
the production `cfg(not(test))` code paths, not ink's offchain test bypasses.
A separate v5 failure probe checks explicit traps, nested domain rejections and
an endless storage-writing loop, including rollback after gas exhaustion.
The marketplace fixture exercises **legacy** contract behavior to establish VM compatibility;
it does not establish parity with the current application or authorize switching.

The committed `fixtures/manifest.json` pins canonical source commit, toolchain,
artifact hashes and per-contract dependency locks. Tests verify artifact and
lock hashes before instantiation. With cargo-contract 5.0.3, Rust 1.88.0 and its
WASM target installed and the manifest source commit available locally, reproduce the WASM and ABI from a local canonical checkout:

```sh
python3 tools/ink-sandbox/rebuild.py ../contracts --offline
```

Omit `--offline` to fetch missing locked build dependencies. This builds committed
source in a disposable directory and compares the resulting WASM hash and ABI;
it never edits the canonical checkout or refreshes committed artifacts. Native
proof tests use Rust 1.96.1. Contract compilation is a separate artifact-build
step, never part of provider startup.

The runtime follows pallet-contracts 43's coherent dependency family rather than
stock ink_sandbox 5.1.1's FRAME 32 family. Its minimal asset extension implements
Kreivo extension 0, functions 0/1/2 (`Here(u32)` only), including exact u128/SCALE
encoding, caller/address semantics, error status codes and `Preserve` transfers.
Other extensions/assets fail explicitly. It does not emulate Kreivo's community
upload permissions, XCM, block consensus or production transaction fees.

Native execution/storage balances are technical fixture funds. Asset 1 balances
are separate. In particular, `Preserve` requires a minimum asset balance; the
legacy Project deliberately leaves one asset unit after payout. The current
application permits spending the full visible balance. Candidate contracts and
the bridge must solve this through the approved separate technical-reserve
policy before money parity can pass; silently reducing user payouts is forbidden.

Checkpoint format here is proof-only. Provider integration still requires a
bundle/runtime identity, bounded decoding, validated roots, single-writer worker,
SQLite atomicity, migration and fault injection. `NoRandomness` is a placeholder
for unused v5 host functionality, not a source for contract matching entropy.
Host-call weights are finite conservative proof limits, not measured production
weights. No performance target or live-chain compatibility is established by a
successful unit suite.

See [dependency review](dependencies.md) and the recorded implementation evidence
in [the handoff](../../progress/handoffs/SPEC-0013.md) before promoting this runtime into
the provider.
