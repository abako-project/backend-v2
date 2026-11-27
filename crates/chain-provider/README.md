# chain-provider

The contract provider's private interface, served from a fork of Kreivo. It is
part of [moving the backend to Kreivo](https://github.com/abako-project/backend-v2/issues/46).

The API talks to two private services: the wallet, for accounts and signatures,
and the contract provider, for everything about the Dao and Project contracts.
With `KREIVO_URL` set, the API opens this crate instead and answers its calls
to the provider, and its requests for signatures, from it, in its own process.
The API's business call sites and its operation pipeline are the same in both
modes. Identity is not: registration, login, password changes and passkeys
call `Provider` directly.

## What it does

- **First start.** It forks a snapshot of Kreivo, brings the fork's clock to
  today, and sets up what the embedded provider sets up: the Dao with its
  catalog, the Project code, the payment asset and the Dao's accounting. The
  operator's pass account is registered, funded and then deploys the Dao, which
  makes it the Dao's operator.
- **Commands.** A command is routed to its contract message, dry-run, and, if
  the contracts accept it, submitted in a transaction signed by the session key
  of the account it comes from. Its receipt and its event are recorded as the
  provider records them. A command the contracts refuse gets a failed receipt
  and is not submitted.
- **Reads.** The snapshot and the other reads are decoded from the contracts
  on the fork's head, on the chain thread, so each read sees one state.
- **Accounts.** An account is a pallet-pass account. `register` creates one
  with its first device and funds it, in one transaction; `open_session` lets
  a device open the session whose key signs that account's commands;
  `replace_device` swaps a device, as a password change does.
- **Passkeys.** `add_passkey` adds a passkey to an account as a device that
  may call contracts and open sessions, authorized by the account's admin
  device; `passkey_login` and `open_passkey_session` open a session with the
  passkey's answer, which the chain verifies. The `passkeys` module checks
  what a browser returns before anything is submitted: its kind, the
  challenge, the page it was made on, and that the user was verified.
- **Fees.** With a community ID in its configuration, the first start also
  sets up that community on the fork with a usage subscription, and every
  account that registers is admitted to it. The community's pool then pays
  the accounts' fees, as on Kreivo; what a call stores still takes its
  deposit from the account's balance. The setup and the admissions use calls
  no account could sign, which stand in for governance, the community's
  admin and the onboarding proxy. An account that cannot be admitted is
  still registered and pays its own fees from then on: the admission is not
  tried again. The community is part of the saved state: a fork is resumed
  only with the one it was set up with. The setup needs a snapshot in which
  Kreivo's declared capacity for usage subscriptions, if any, has room for
  the offer.
- **Sessions.** A fork's clock moves six seconds with every block, so a
  session's 150 relay blocks would pass in 150 transactions. The provider
  cancels the removal the chain schedules and ends each session itself, 15
  minutes after it opened by the wall clock. A new session of an account ends
  its earlier one. A session's key is noted in the saved state before the
  session is opened: the chain frees a session's place only when asked, so a
  session nobody remembered would hold its place for good.
- **Writes run to their end.** Every write to the chain is finished even if
  whoever asked for it stops waiting, so a transaction that was sent leaves
  its mark in the saved state. At most 64 account writes (registrations,
  sessions, devices) run or wait at once; one more is refused with
  `capacity_exceeded`. Commands are bounded by the 64 requests the interface
  takes at once.
- **The wallet's endpoints.** A signing request succeeds, with a placeholder,
  exactly when the account has a live session, and is rejected otherwise. The
  system wallet is the operator's account.

The command routing, the fact decoding and the shell that keeps nonces,
receipts and events are copies of those in `services/contract-provider`.

## What it keeps

In the data directory:

| File | Holds |
|---|---|
| `fork.snap` | The fork's head and state, saved after every command and every account transaction that succeeds |
| `state.json` | The record's layout version, the provider's nonces, receipts and events, the Dao's address, the community's ID, and which sessions are open (not their keys) |
| `onboarding.seed` | The key that registers and funds accounts on the fork |

Session keys are in memory only. A restart ends every session, on the chain
too.

## Limits

- A fork only. Reads take the fork's state in place; a node needs paged reads
  through contract messages.
- One process. Whoever executes a command is the only writer to the chain.
- Nothing records a command before it is submitted. A submission that ends
  in an error that does not say whether it was included is reported as
  unavailable, and a stop between saving the fork and saving the state
  leaves a transaction in the fork without its receipt. In both cases the
  API sends the command again: it then runs a second time, or the contracts
  refuse it and that refusal is recorded for a command that took effect.
- An account transaction the chain includes and fails is not saved to
  `fork.snap` until the next save.
- The chain's time can run ahead of the wall clock after a burst of
  transactions, and receipts carry the chain's time.
- A login with a passkey is for one passkey, the one registered last.
- The first start takes about a minute with a week-old snapshot: the runtime's
  scheduler works through every relay block the clock skips.

## Verification

```sh
KREIVO_FORK_SNAPSHOT=/path/to/kreivo.snap \
  cargo test -p chain-provider --features fork --release -- --include-ignored
```

One test starts the provider on a fork, registers a pass account, opens its
session, executes a command, reads it back, replaces the device and starts
the provider again from the saved state. Another adds a software passkey,
logs in with it and removes it. A third compares what a command costs an
account with and without a community. Without a snapshot they are ignored.
