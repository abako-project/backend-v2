# Data Model

## Owning Bounded Context

The custody bounded context, initially implemented by `services/wallet`, owns wallets, encrypted seeds, signing jobs, leases, signatures, and custody audit records.

The adapter owns principals, sessions, authorization, and provider-operation status. The mock owns account nonces, execution receipts, contract state, and durable provider events.

## Aggregates and Invariants

### Custodial wallet

- One wallet per `PrincipalId`.
- One principal per wallet.
- One signing scheme per wallet; initially `Sr25519`.
- Only `Active` signs.
- `Suspended` and `Retired` retain ciphertext and history.
- The system-origin wallet has no human principal.

### Signing job

- One payload hash per `OperationId`.
- At most one active lease per wallet.
- `Signed` and `Rejected` are terminal.
- A signature is written only by the worker that still owns the lease.

### Mock execution

- An account accepts only its next nonce.
- An exact duplicate returns its receipt.
- A conflicting duplicate fails.
- State, nonce, receipt, and event commit together.

## Domain Types

| Type | Representation | Rules |
|---|---|---|
| `WalletId` | Opaque fixed-width identifier | Generated once by custody |
| `PrincipalId` | Cross-service opaque identifier | Unique in custody except for the system origin |
| `OperationId` | Opaque 16-byte identifier | Idempotency identity |
| `AccountId32` | 32 bytes | Derived from the `sr25519` public key |
| `PayloadVersion` | Unsigned integer newtype | Initially version 1 |
| `UnixSeconds` | Unsigned 64-bit integer newtype | UTC instant |
| `PayloadHash` | 32 bytes | SHA-256 over the exact signable bytes |
| `WalletLifecycle` | `Provisioning`, `Active`, `Suspended`, `Retired` | Forward transitions only |
| `SigningJobStatus` | `Pending`, `Signed`, `Rejected` | Terminal after signing or rejection |
| `ProviderOperationStatus` | `AwaitingSignature`, `ReadyToSubmit`, `Submitted`, `OutcomeUnknown`, `Finalized`, `Rejected`, `Expired` | Controlled transitions; finality and execution outcome are distinct |

## SQLite Tables and Constraints

### `custodial_wallets`

| Column | Constraint |
|---|---|
| `wallet_id` | Primary key |
| `principal_id` | Unique; nullable only for the system origin |
| `account_id` | Unique, 32 bytes |
| `scheme` | Checked supported value |
| `encrypted_seed` | Non-empty blob |
| `encryption_nonce` | Unique with master-key version; exactly 24 bytes |
| `encryption_format_version` | Positive integer; initially XChaCha20-Poly1305 format 1 |
| `master_key_version` | Positive integer |
| `lifecycle` | Checked enum value |
| `created_at`, `updated_at` | Unix seconds |

### `signing_jobs`

| Column | Constraint |
|---|---|
| `operation_id` | Primary key |
| `creation_sequence` | Unique monotonic insertion sequence; queue order does not depend on timestamp precision or random IDs |
| `wallet_id` | Foreign key to custody-owned wallet |
| `payload_version` | Positive integer |
| `signable_payload` | Non-empty bounded blob |
| `payload_hash` | 32 bytes; immutable |
| `expires_at` | Unix seconds |
| `status` | Checked enum value |
| `signature` | Present only when `Signed` |
| `attempt_count` | Non-negative integer |
| `next_attempt_at` | Unix seconds or null |
| `lease_token` | Unique opaque value or null |
| `lease_until` | Present exactly when `lease_token` is present |
| `rejection_code` | Present only when `Rejected` |
| `created_at`, `updated_at` | Unix seconds |

A partial unique index on `wallet_id` where `lease_token` is not null enforces one claimed job per wallet. Queue selection orders pending jobs for a wallet by creation sequence and operation ID. Admission rejects a thirty-third pending job for one wallet or any job beyond 10,000 pending jobs in the instance.

### `custody_audit_records`

| Column | Constraint |
|---|---|
| `audit_id` | Primary key |
| `operation_id` | Nullable indexed reference |
| `wallet_id` | Nullable indexed reference |
| `event_kind` | Stable checked value |
| `result_code` | Stable checked value |
| `payload_hash` | Nullable; never raw payload |
| `occurred_at` | Unix seconds |
| `details` | Bounded redacted structured data |

Adapter and mock tables belong to their own specifications. They do not reference custody tables with cross-service foreign keys.

## Indexes and Query Shapes

- Unique wallet lookup by `principal_id` and `account_id`.
- Signing job lookup by `operation_id`.
- Pending scan by `status`, `next_attempt_at`, and creation order.
- Partial uniqueness for one non-null lease per wallet.
- Audit lookup by `operation_id`, `wallet_id`, and occurrence time.

## Transaction Boundaries

Wallet provisioning generates and encrypts a seed before a transaction inserts the wallet. A uniqueness conflict reads and returns the existing wallet without replacing its key.

Job creation inserts or compares the existing exact payload bytes and hash in one transaction. Claiming assigns a lease conditionally while respecting per-wallet creation sequence. Completion updates a job only when its lease token still matches.

The mock verifies the envelope and writes domain state, next nonce, receipt, and durable event in one transaction in its own database. No transaction crosses a service or database boundary.

The signed envelope binds a fixed signing domain and a provider instance ID. A fresh instance ID is generated when mock state is reset. The adapter retains the instance ID with every operation and never silently retargets signed bytes after a reset. An authenticated exact duplicate is reconciled before expiration or next-nonce validation for new execution.

## Migration Plan

The initial custody migration creates all custody tables and constraints before traffic. The initial mock migration adds nonce and receipt storage before unsigned calls are disabled. Existing mock data requires no backfill.

The POC records `master_key_version = 1` but does not rotate it. Before real
value, a separately approved HSM-backed design must rotate encryption keys
through a resumable, verified process while preserving wallet accounts.
Changing a signing seed or account scheme is a separate public-identity
migration. No existing POC row should be treated as already HSM-wrapped.

## Backfill and Verification

There is no initial backfill. Migration verification checks schema constraints, unique indexes, ciphertext decryptability with the active master-key set, and equality between stored account IDs and public keys derived from decrypted seeds. Verification must not log decrypted data.

## Retention, Deletion, and Privacy

Wallet ciphertext and lifecycle history are not deleted while the account identifies provider activity. Local payload and audit retention has not been fixed. Production retention must be approved before real funds or personal data are processed.

## Read Models and Cross-Service Replication

The adapter may store wallet ID, account ID, lifecycle summary, and signing-operation status. It never replicates ciphertext, seeds, master-key metadata, or service secrets. The mock stores the origin in each accepted call but does not read custody data.
