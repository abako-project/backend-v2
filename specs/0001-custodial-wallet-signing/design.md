# Technical Design

## Context

The adapter serves people who authenticate with classic credentials but must cause real signed provider operations. The proof of concept uses a mock provider. The future provider uses Subxt against a Polkadot SDK runtime. Key custody is a security boundary, while transaction construction and submission remain provider concerns.

## Decision Summary

- Full custody only; no browser export or self-custody.
- One `sr25519` `AccountId32` wallet per classic principal.
- The wallet service becomes the custody bounded context and owns key material.
- The authenticated application action authorizes its signature without a second prompt.
- Durable signing jobs use internal HTTP and polling, not a broker or webhook.
- Custody signs but never submits provider calls.
- Operations are serialized per wallet and concurrent across wallets.
- The mock verifies versioned SCALE calls and commits each call atomically.
- Login credentials never derive or encrypt the signing seed.
- The legacy browser-facing arbitrary-signing endpoint is not ported.

## Legacy Findings

The current branch was reindexed as `kunveno-task-storages-current` from commit `d408641`. The new design deliberately replaces these behaviors:

- `AuthController.sign` exposes a browser-facing signing action, and `SignRequest.extrinsic` accepts `any`. The Rust adapter will expose typed domain mutations only.
- `AuthService.sign` sends caller-provided extrinsic data to an external token signer and logs fragments of both token and extrinsic. The Rust path will log neither.
- `MockAuthService.sign` returns random transaction and block hashes without producing a signature. The Rust mock will verify a real `sr25519` signature.
- `deriveKeyPair` derives an Ed25519 private key from the user's password. Custodial wallet generation will use independent secure randomness, so credential changes preserve the wallet.
- Calendar calls currently craft an unsigned extrinsic and then invoke the authentication signer, while some project calls use a separate pre-signed path. The Rust provider worker will use one durable signing pipeline.
- `external/subskribinto` demonstrates `sr25519`, `AccountId32`, runtime metadata, and Subxt submission, but combines signing and submission and uses a custom `sp_core` signer. It is evidence, not code to port into custody.

## Components and Responsibilities

### Adapter API

- Authenticates the browser session.
- Authorizes the requested domain action.
- Creates an immutable provider operation.
- Exposes operation progress through its browser contract and existing SSE path.

### Provider worker and gateway

- Converts the authorized domain operation into a provider-specific call.
- Selects and reconciles the next wallet nonce.
- Requests a signature from custody.
- Stores the signed bytes before submission.
- Submits to the mock or future chain and reconciles the receipt.

### Custody service

- Provisions one wallet per principal.
- Encrypts and owns private seeds.
- Authenticates internal callers.
- Persists, leases, validates, and signs signing jobs.
- Returns signatures without contacting the provider.

### Mock provider

- Decodes the signed call.
- Verifies its version, signature, origin, nonce, expiration, and idempotency.
- Dispatches the message to the addressed mock contract instance.
- Atomically stores state, nonce, receipt, and event.

## Data and Control Flow

1. A principal performs an authorized action through the adapter.
2. The adapter records an immutable provider operation.
3. The provider worker claims the next operation for that wallet.
4. The gateway builds `UnsignedContractCallV1` and records its exact bytes and hash.
5. The worker creates an idempotent signing job in custody.
6. A custody worker leases the job, validates it, decrypts the active wallet seed, signs the bytes, clears plaintext key material, and stores the signature.
7. The provider worker reads the signature and stores `SignedContractCallV1` before sending it.
8. The mock verifies and applies the call in one transaction.
9. The provider worker reconciles the receipt. The adapter later ingests the committed event for SSE delivery.

## Contracts

The conceptual signed types are:

```rust
struct UnsignedContractCallV1 {
    payload_version: PayloadVersion,
    operation_id: OperationId,
    origin: AccountId32,
    contract: ContractRef,
    message: MessageRef,
    nonce: u64,
    expires_at: UnixSeconds,
    payload: Vec<u8>,
}

struct SignedContractCallV1 {
    call: UnsignedContractCallV1,
    signature: Sr25519Signature,
}
```

`UnsignedContractCallV1` is SCALE encoded once. Custody signs those exact bytes. `ContractRef`, `MessageRef`, and payload formats belong to versioned provider contracts rather than the custody domain.

The custody API accepts the operation ID, wallet ID, payload version, signable bytes, payload hash, and expiration. It returns job state and, only when signed, the public account and signature. It never returns a seed, mnemonic, private key, or master-key data.

## Failure and Retry Model

Signing jobs use `Pending`, `Signed`, and `Rejected`. Provider operations use `AwaitingSignature`, `ReadyToSubmit`, `Submitted`, `Finalized`, `Rejected`, and `Expired`.

Leases are metadata on pending work, not a durable business state. A worker may update a leased row only when its lease token still matches. Expired leases may be reclaimed. Transient failures schedule a bounded retry at the current stage. Terminal validation failures become `Rejected`.

The provider persists signed bytes before network submission. A transport retry resends identical bytes. A duplicate successful mock submission returns the stored receipt. A conflicting `OperationId` never executes.

## Concurrency and Backpressure

The provider admits one signing/submission critical section per wallet. It does not block unrelated wallets. Custody uses a lease token and enforces at most one claimed job per wallet. The local bounds are 256 KiB per signable payload, 32 pending jobs per wallet, 10,000 pending jobs per instance, a 5-minute call lifetime, a 30-second lease, five attempts per stage, backoff from 1 to 30 seconds, and a 5-second internal request timeout.

## Security Controls

- Dedicated custody database and process boundary.
- XChaCha20-Poly1305 through RustCrypto `chacha20poly1305` 0.11 with `zeroize`.
- A fresh 24-byte nonce and bound associated data per wallet.
- A 32-byte master key and versioned ciphertext format.
- Runtime master secret absent from database and source control.
- Internal-only endpoint plus service authentication.
- Authorization before job creation and policy validation before signing.
- Account origin derived from the signing public key.
- Signed version, target, message, nonce, expiration, and payload.
- Secret wrappers and memory clearing where supported by reviewed dependencies.
- Structured audit records and telemetry redaction.
- Separate system-origin wallet.

The proof of concept accepts that compromise of the running custody process or its master key permits fraudulent signatures. Real assets require KMS or HSM integration, workload identity, backups, incident procedures, and a production threat review.

## Observability

All stages correlate on `OperationId`. Logs and traces record identifiers, stage changes, attempt numbers, durations, and stable error classes. Metrics expose queue depth, oldest pending age, recovered leases, signing outcomes, submission outcomes, and retry exhaustion. Raw signed payloads and all secrets are excluded.

## Deployment and Compatibility

The custody SQLite database is separate from the mock database. Only internal callers can reach its API. Local runtime secrets are injected outside source control.

The mock gateway is disposable. A future Subxt gateway will construct the runtime-specific signer payload and submit an extrinsic, while custody continues to protect the same `sr25519` seed and sign approved bytes. H160 or ECDSA requires a separate wallet migration design.

## Dependency Decision

As reviewed on 2026-09-02, RustCrypto `chacha20poly1305` 0.11.0 provides XChaCha20-Poly1305, uses a 32-byte key and 24-byte extended nonce, is pure Rust, is licensed `Apache-2.0 OR MIT`, declares Rust 1.85, and reports an NCC Group audit without significant findings. The project MSRV is 1.96.1. Use `default-features = false` with only `alloc`, `getrandom`, and `zeroize`; do not enable reduced-round variants.

Sources: [crate documentation](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/) and [published Cargo metadata](https://docs.rs/crate/chacha20poly1305/0.11.0/source/Cargo.toml).

## Alternatives Considered

### Sign synchronously in the adapter

Rejected because it exposes the adapter process to keys and makes request failures difficult to reconcile.

### Let custody sign and submit

Rejected because it couples key custody to mock and runtime-specific networking.

### Use RabbitMQ for signing jobs

Rejected for the proof of concept. A durable custody table and polling provide the required recovery with fewer moving parts.

### Export encrypted wallets to browsers

Rejected from the initial scope. Browser decryption changes the product from full custody to a hybrid or self-custody model and increases the attack surface.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Database copied | Authenticated ciphertext; master key stored separately |
| Ciphertext moved between wallets | Wallet and principal identity included in associated data |
| Unintended signature requested | Typed operation, prior authorization, immutable hash, custody policy, audit trail |
| Duplicate or reordered execution | Operation ID, account nonce, per-wallet serialization, atomic receipt |
| Worker dies while signing | Expiring lease and conditional update |
| Provider reply is lost | Persist signed bytes and return stored receipt for exact duplicates |
| Secrets leak through diagnostics | Redaction tests and no raw payload or key logging |
| Custody process or master key is compromised | Suspend wallets; require external key protection before real funds |

## Validation Plan

- Verify every acceptance scenario.
- Run the unit, property, integration, contract, end-to-end, concurrency, and security tests in `spec.md`.
- Recheck the selected dependency version, features, license, MSRV, unsafe or native code, and test vectors immediately before adding it.
- Independently review custody authorization, key handling, and telemetry before enabling signing.
