# kreivo

The backend's client for Kreivo, built on
[sube](https://github.com/virto-network/virto-sdk/tree/main/crates/sube). It is
part of [moving the backend to Kreivo](https://github.com/abako-project/backend-v2/issues/46). The API
uses it through [chain-provider](../chain-provider/README.md) when it runs the
contracts on a fork.

One thread owns the connection, because sube is single-threaded. `Chain`
handles are `Send`, queue at most 32 requests to it, and wait two minutes for an
answer. The connection string selects the chain:

| `Chain::connect` | Feature | Chain |
|---|---|---|
| `wss://…` | `live` (default) | A Kreivo node |
| `fork:<snapshot>` | `fork` | An in-process fork of Kreivo with instant blocks |

A fork runs the real runtime, so pallet-pass, contract limits and fees behave
as on the node. `Chain::mock()` adds what no account can do on Kreivo:

- create balances and the payment asset, upload and instantiate contracts;
- move the clock, or bring it to the wall clock;
- read contract storage and run contract messages in place, on the chain
  thread, so that many reads see one state;
- keep a session past its relay blocks and end it, as Root;
- dispatch any call as Root or as an account, without a signature
  (`dispatch`), and read an account's devices, a storage value or a balance;
- save the fork to boot it again.

A live image must not enable `fork`.

## What it does

- **Contracts.** `dry_run` runs a message through `ContractsApi_call` and
  returns its gas, deposit and return data; `call` submits `Contracts.call` and
  waits for finality. `message` builds an ink! input. The crate knows no
  marketplace rule or contract address. A dry-run is allowed far more gas than
  a transaction: compare what it reports with the chain's limits.
- **Pass accounts.** `enrollment` and `register` create a pass account for a
  key device, and `register_funded` also funds it in the same transaction;
  `session_request` and `open_session` let that device authorize a session
  key limited to `Contracts.call`; `replacement_request` swaps a device for
  another in one batch, and `removal_request` removes one. Each is two steps,
  so that a device can sign elsewhere.
- **Passkeys.** `passkey_addition_request` adds a WebAuthn device with a
  filter, authorized by a key device; `passkey_session_request` and
  `open_passkey_session` open a session with the passkey's assertion. The chain verifies every signature: a successful
  receipt is the proof. A session lasts 150 relay chain blocks, 15 minutes on
  Kreivo; afterwards the chain rejects what its key signs, as it does a key
  that never had a session.
- **Authority.** A transaction is authorized by a `Key` this process holds, the
  operator's, or by a session key acting for a pass account, which stays the
  contract's caller and pays.

A `Receipt` is returned for a failed dispatch too, since its fee was paid. An
`Error` means the transaction was not included or its outcome is unknown.

## Limits

- Passkeys were run with a software authenticator only, not a browser.
- A request that times out is not cancelled and may still complete. There is no
  recovery of a submission interrupted by a restart yet.
- Nothing follows finalized blocks for notifications yet.
- A saved fork holds its head and state only. Save it when the process stops;
  a crash loses the blocks since the last save.
- A fork builds every block as the first of a core, so it accepts
  transactions that need a whole core in any block. Kreivo does in one of three.
- A fork's clock moves one relay block, six seconds, with every block it
  builds. A burst of transactions leaves it ahead of the wall clock, and a
  session's 150 relay blocks pass in 150 blocks: hence `keep_session`.

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

sube, pass, libwallet and scale-serialization, and with `fork` sube-fork and
smoldot, are GPL-3.0-or-later (smoldot with the Classpath exception). This
crate's source is Apache-2.0; a binary that links it is distributed under the
GPL. `deny.toml` carries the exceptions. The workspace pins a revision of
virto-sdk's `main`.

Keys are libwallet's sr25519 pairs.
