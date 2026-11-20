# kreivo

The backend's client for Kreivo, built on
[sube](https://github.com/virto-network/virto-sdk/tree/main/crates/sube). It is
stage 1 of [SPEC-0016](../../specs/0016-kreivo-chain-backend/spec.md): nothing
in the API uses it yet, and the wallet and contract-provider services are
unchanged.

One thread owns the connection, because sube is single-threaded. `Chain`
handles are `Send`, queue at most 32 requests to it, and wait two minutes for an
answer. The connection string selects the chain:

| `Chain::connect` | Feature | Chain |
|---|---|---|
| `wss://…` | `live` (default) | A Kreivo node |
| `fork:<snapshot>` | `fork` | An in-process fork of Kreivo with instant blocks |

A fork runs the real runtime, so pallet-pass, contract limits and fees behave
as on the node. `Chain::mock()` adds what no account can do on Kreivo: create
balances, upload and instantiate contracts, move the clock, and save the fork
to boot it again. A live image must not enable `fork`.

## What it does

- **Contracts.** `dry_run` runs a message through `ContractsApi_call` and
  returns its gas, deposit and return data; `call` submits `Contracts.call` and
  waits for finality. `message` builds an ink! input. The crate knows no
  marketplace rule or contract address. A dry-run is allowed far more gas than
  a transaction: compare what it reports with the chain's limits.
- **Pass accounts.** `enrollment` and `register` create a pass account for a
  key device; `session_request` and `open_session` let that device authorize a
  session key limited to `Contracts.call`. Each pair is two steps because the
  device signs in a browser. The chain verifies every signature: a successful
  receipt is the proof. A session lasts 150 relay chain blocks, 15 minutes on
  Kreivo; afterwards the chain rejects what its key signs, as it does a key
  that never had a session.
- **Authority.** A transaction is authorized by a `Key` this process holds, the
  operator's, or by a session key acting for a pass account, which stays the
  contract's caller and pays.

A `Receipt` is returned for a failed dispatch too, since its fee was paid. An
`Error` means the transaction was not included or its outcome is unknown.

## Limits

- Key devices (`SubstrateKey`, sr25519) only. WebAuthn devices are not wired.
- A request that times out is not cancelled and may still complete. There is no
  recovery of a submission interrupted by a restart yet.
- Nothing follows finalized blocks for notifications yet.
- The mock's payment asset is not created here yet. The spike shows how: one
  storage write of `Assets.Asset` and a mint by the operator.
- A saved fork holds its head and state only. Save it when the process stops;
  a crash loses the blocks since the last save.
- A fork builds every block as the first of a core, so it accepts
  transactions that need a whole core in any block. Kreivo does in one of three.

## Verification

```sh
cargo test -p kreivo
cargo clippy -p kreivo --all-targets --all-features -- -D warnings
```

The fork tests need a full-state snapshot of Kreivo, about 25 MB, so they are
ignored by default. Make one with sube's `fork_snapshot` example, then:

```sh
KREIVO_FORK_SNAPSHOT=/path/to/kreivo.snap \
  cargo test -p kreivo --all-features --release -- --include-ignored
```

They deploy the pinned Dao and Project artifacts from
`services/contract-provider/fixtures`.

## Dependencies

sube, pass and scale-serialization, and with `fork` sube-fork and smoldot, are
GPL-3.0-or-later. This crate's source is Apache-2.0; a binary that links it is
distributed under the GPL. `deny.toml` carries the exceptions. The workspace
pins the head of the virto-sdk branch `feat/fork-dev-ops`, which adds what the
mock needs; the pin moves to the commit on `main` once that branch merges.
