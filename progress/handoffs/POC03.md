# POC03: Custodial signing service

Status: Implemented; ready for integration verification.

Specification: APPROVED SPEC-0001, shared generated contracts and ADR-0001.
Branch: `feat/poc-custody`.
Worktree: `historical custody worktree`.
Write scope: `services/wallet/**` and this handoff. The integrator owns the lockfile.

## Delivered

- Independent SQLite custody storage and constrained schema; encrypted wallet seeds
  use XChaCha20-Poly1305 with wallet/principal/version-bound authenticated data.
- Stable principal wallets and separate system wallet. Startup verifies encrypted
  identities and fails on wrong keys, changed root identity or damaged ciphertext.
- Authenticated internal provisioning and asynchronous durable signing jobs;
  exact-byte and metadata idempotency, canonical SCALE validation and SHA-256.
- Four observed signing workers; ordered per-wallet claims, expiring leases,
  conditional completion, queue limits and rejection of inactive wallets.
- Real sr25519 signatures, no provider submission client or private-key export.
- Secret-free audit rows and internal metrics; private development-secret creation
  and restricted local lifecycle CLI. Runtime configuration is in the service README.

## Verification (2026-09-10)

All commands ran from this worktree with `rtk proxy`:

| Command after `rtk proxy` | Observed result |
|---|---|
| `cargo fmt -p wallet -- --check` | Pass |
| `cargo check -p wallet --all-targets --all-features --offline --locked` | Pass |
| `cargo clippy -p wallet --all-targets --all-features --offline -- -D warnings` | Pass; inherited root MSRV configuration warning only |
| `cargo test -p wallet --offline` | 9 passed |
| `cargo nextest run -p wallet --offline` | 9 passed with local-loopback permission |
| `cargo build -p wallet --all-targets --all-features --offline --locked` | Pass |
| `git diff --check` | Pass |

The first sandboxed Nextest run passed 8 tests and failed the HTTP test solely
because binding an ephemeral loopback port returned EPERM. The unchanged command
passed all 9 after the network permission was granted.

Tests cover ciphertext substitution/tampering, malformed payloads, wrong hash,
origin/version/expiration, exact retries and conflicts, queue admission, restart
durability, lease recovery and stale completion, per-wallet ordering and other-wallet
progress, inactive wallets, attempt exhaustion, private create-new secret files,
authenticated HTTP status polling and redacted error output.

## Integration notes and limits

- `/health` and `/ready` return 204 on the private listener. Do not proxy them publicly.
- Lockfile changes remain local and uncommitted for the integrator to regenerate.
- Root `clippy.toml` in this branch still declares 1.85.0; the integrator already
  owns the correction to the workspace MSRV 1.96.1.
- Whole-workspace supply-chain gates and adapter/provider/browser end-to-end checks
  belong to integration; the focused checks above do not claim those passed.
- A single SQLite connection serializes short writes, while signing executes
  outside the transaction. This is deliberate for the low-load POC.
- No production assets, key rotation, browser key export, KMS/HSM or production
  recovery implementation. Host/process owners remain trusted.

Blocker: none for this scope.
