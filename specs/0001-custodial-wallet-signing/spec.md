# SPEC-0001: Custodial wallet signing

Status: APPROVED

Owner: Kunveno product owner

Created: 2026-09-02

Last updated: 2026-09-08

Target milestone: Rust proof of concept

## Problem

People who use classic login credentials need to perform operations whose source of truth is the mock provider and, later, a blockchain. They must not manage blockchain keys. The backend therefore needs to provision wallets, protect their private keys, authorize signatures, and submit signed calls without exposing key material.

## Goals

- Give each classic principal one stable custodial `AccountId32`.
- Keep private keys inside a dedicated custody process.
- Make the mock accept only authentic, fresh, idempotent signed calls.
- Recover safely from process and network failures without duplicating an operation.
- Replace the mock provider with a Subxt blockchain provider without changing browser authentication or domain use cases.

## Non-Goals

- Exporting or decrypting private keys in a browser.
- Self-custody or external-wallet login.
- H160 accounts, `pallet-revive`, or Ethereum transaction encoding.
- Automatic replacement of a compromised signing seed.
- Production KMS or HSM integration during the proof of concept.
- Migrating mock storage or mock implementation code into production.
- Preserving the legacy public `POST /v1/auth/sign` contract.

## Actors

| Actor | Description | Permissions |
|---|---|---|
| Classic principal | A client, worker, or coordinator authenticated by the adapter | Request operations allowed by the principal's current permissions |
| Adapter API | Browser-facing authentication and application boundary | Authenticate principals and create authorized provider operations |
| Provider worker | Adapter-owned background processor | Prepare calls, request signatures, submit calls, and reconcile results |
| Custody service | Isolated owner of wallets and signing jobs | Provision wallets and sign approved payloads with active keys |
| Mock provider | Development substitute for the future chain | Verify and execute signed contract calls atomically |
| System origin | A separate privileged system wallet | Perform operations explicitly reserved for platform authority |

## Definitions

- **Principal:** Authentication identity represented by `PrincipalId`.
- **Custodial wallet:** An `sr25519` keypair whose private seed is controlled exclusively by the custody service.
- **Provider operation:** An authorized attempt to change provider state.
- **Signing job:** A durable request for the custody service to sign one immutable payload.
- **Signed contract call:** A versioned mock call whose SCALE-encoded content is bound to an `sr25519` signature.
- **In flight:** A provider operation that has not reached `Finalized`, `Rejected`, or `Expired`.

## Functional Requirements

### REQ-001: Provision one wallet per classic principal

The custody service shall create exactly one custodial wallet for each `PrincipalId`. Repeating the provisioning request shall return the existing wallet. The initial scheme shall be `sr25519`, and its public key shall define the principal's `AccountId32`.

### REQ-002: Protect private key material

The custody service shall store each seed with XChaCha20-Poly1305 from RustCrypto's `chacha20poly1305` crate. Each wallet shall use a fresh 24-byte random nonce and record the encryption-format and master-key versions. Associated authenticated data shall bind the ciphertext to the wallet, principal, signing scheme, and both versions. The decoded master key shall contain exactly 32 bytes, be provided at process startup through a runtime secret, and never be stored in SQLite. Startup shall fail when the required key is absent or invalid. The implementation shall enable the crate's `zeroize` support.

### REQ-003: Authorize signing through the authenticated action

An authenticated application action shall be sufficient user authorization for its resulting signature. The system shall not request a second confirmation. The adapter shall enforce the principal's permissions before creating the provider operation.

### REQ-004: Use durable, idempotent signing jobs

The provider worker shall create a signing job through the custody service's internal API. A successful creation shall return `202 Accepted`. SHA-256 over the exact signable bytes shall be the payload hash. Repeating the same `OperationId` with identical bytes shall return the existing job. Reusing it with different bytes shall be rejected even if a hash comparison were to collide. The provider worker shall query the job by `OperationId`; no webhook or message broker is required.

### REQ-005: Separate signing from submission

The custody service shall validate and sign the immutable payload but shall not contact the mock or blockchain provider. For the proof of concept it shall decode the supported mock payload version and verify its operation ID, origin, expiration, and payload hash against the signing job. Unknown payload kinds or versions shall be rejected. The provider worker shall submit the signed call and track its result. It shall not be able to alter signed content. A future Subxt payload kind requires its own reviewed validator.

### REQ-006: Serialize operations per wallet

At most one provider operation per wallet shall be in the signing or submission critical section. Operations for one wallet shall preserve order. Different wallets may progress concurrently.

### REQ-007: Sign a versioned mock envelope

The mock call shall contain a fixed mock-signing domain, provider instance ID, `PayloadVersion`, `OperationId`, `AccountId32` origin, contract instance, message, nonce, Unix expiration time, and payload. The provider instance ID shall change whenever disposable mock state is reset, and remain stable while that state is retained. The custody service shall sign the complete SCALE encoding of the unsigned call. The signed form shall contain the unchanged call and its `sr25519` signature. A call for another domain or provider instance shall never execute.

### REQ-008: Verify and execute atomically

The mock provider shall verify the signing domain, provider instance, encoding version, signature, and origin. It shall then check for an existing receipt bound to identical unsigned call bytes before applying freshness and next-nonce checks to a new execution. An authenticated exact replay shall return its receipt even after the original expiration. Contract authorization shall be checked in the provider, not entrusted solely to the adapter. Successful execution shall commit all affected contract state, the incremented account nonce, receipt, and durable events in one transaction. Preparing signable bytes shall have no business side effects.

### REQ-009: Recover bounded background work

A signing job shall be `Pending`, `Signed`, or `Rejected`. A provider operation shall be `AwaitingSignature`, `ReadyToSubmit`, `Submitted`, `OutcomeUnknown`, `Finalized`, `Rejected`, or `Expired`. Workers shall claim work with expiring leases. A transient failure shall retain the current state, increment the attempt count, and schedule a bounded retry. A terminal validation failure shall produce `Rejected` only when non-execution is established. Submission timeouts and exhausted submission retries shall preserve an unknown outcome until reconciled against provider receipts. Expiration after a possible submission does not prove non-execution. Finalization shall include a separate execution outcome; finalization alone does not establish business success.

### REQ-010: Enforce wallet lifecycle

A wallet shall move from `Provisioning` to `Active`, and may later move from `Active` to `Suspended` or `Retired`. Only `Active` wallets may sign. Suspended and retired key material and history shall remain encrypted and retained. Signing-seed replacement is not automatic.

### REQ-011: Authenticate the internal custody API

The custody API shall be reachable only on the internal deployment network. During local development, callers shall authenticate with a high-entropy service token supplied as a runtime secret. Production shall replace this with workload identity or mutual TLS before real funds are used.

### REQ-012: Record a secret-free audit trail

Wallet provisioning, lifecycle changes, signing requests, signing outcomes, submission outcomes, and authorization failures shall create audit records containing identifiers, hashes, timestamps, and stable result codes. Audit output shall exclude seeds, master keys, plaintext credentials, service tokens, and raw decrypted key material.

### REQ-013: Forbid browser-supplied signing payloads

The browser API shall not accept an extrinsic, raw signing bytes, caller-supplied nonce, or arbitrary contract message for signing. A typed domain endpoint may accept its documented resource identifier, but the adapter shall derive the allowed provider target and message from that endpoint. Only an authorized typed action may create a provider operation and subsequent signing job.

Rationale: Authentication alone must not turn the adapter into an unrestricted signing oracle.

### REQ-014: Keep credentials independent from wallet keys

Passwords and other login credentials shall not derive, encrypt, replace, or determine the custodial signing seed. Changing or recovering login credentials shall preserve the principal's wallet and `AccountId32`.

Rationale: Authentication recovery must not silently change an on-chain identity.

## Invariants

- INV-001: A `PrincipalId` identifies at most one custodial wallet.
- INV-002: Private seeds and master keys never cross the custody process boundary in plaintext.
- INV-003: Only an `Active` wallet may create a signature.
- INV-004: The call origin equals the `AccountId32` derived from the signing public key.
- INV-005: The mock never executes an unsigned, invalid, expired, or out-of-order call.
- INV-006: One `OperationId` is permanently bound to one payload hash.
- INV-007: A wallet has at most one claimed signing job at a time.
- INV-008: A submitted signed payload is immutable and is reused unchanged for transport retries.
- INV-009: The system origin wallet is never assigned to a human principal.
- INV-010: Mock state, nonce, receipt, and event change together or not at all.
- INV-011: No browser request can provide arbitrary bytes to the signing boundary.
- INV-012: A credential change does not change the custodial signing identity.

## Failure Behavior

| Condition | Observable result | Retryable | Audit requirement |
|---|---|---:|---|
| Principal is not authenticated or authorized | Operation rejected before a signing job exists | No | Principal, action, and stable denial code |
| Custody master key is missing or invalid | Custody service does not become ready | No | Startup error without secret values |
| Wallet is not active | Signing job becomes `Rejected` | No | Wallet, operation, and lifecycle state |
| Existing `OperationId` has different content | Request rejected as an idempotency conflict | No | Operation and both payload hashes |
| Signing worker loses its lease | Another worker may reclaim the pending job | Yes | Lease expiration and next attempt |
| Internal network or provider is unavailable | Current stage is retained and retried within its bound | Yes | Attempt, stage, and redacted error class |
| Signature, origin, nonce, or expiration is invalid | Mock call rejected without state mutation | No | Operation and stable validation code |
| Exact finalized call is repeated | Previous receipt returned without another state change | No | Duplicate observation |
| Submission retry budget is exhausted | Operation remains `OutcomeUnknown`; reconcile before another business attempt | Reconciliation only | Attempt count and unresolved outcome |
| Provider instance changes after a possible submission | Preserve the old instance and unresolved history; never replay against the new instance | No automatic replay | Old/new instance IDs and affected operation |

## Acceptance Behavior

Executable scenarios are in `acceptance.feature`.

## Contract Impact

### REST

Typed adapter REST commands return an operation reference and asynchronous status. Both the external frontend and Leptos use the same documented HTTP contract. No Leptos server-function protocol is required. The browser contract exposes neither raw-signature operations nor fields that accept arbitrary signable bytes.

### gRPC

None.

### HTTP and Webhooks

- `POST /internal/signing-jobs`: create or retrieve an idempotent job.
- `GET /internal/signing-jobs/{operation_id}`: read status and signed output when available.
- `POST /internal/contracts/call`: submit a signed call to the mock provider.
- Provider discovery, account nonce, operation receipt, and cursor-based event reads shall be documented before implementing reconciliation.
- No webhooks are introduced.

### Published Events

The mock records durable domain events in the same transaction as successful execution. An adapter-owned ingestor reads them by cursor and commits its cursor together with idempotent notification inserts. SSE reads persisted notifications and supports resume. The legacy in-memory broadcaster is not an existing implementation of this recovery pipeline. Delivery does not mark a notification read.

### Consumed Events

None. Workers poll their owning service's durable state.

### Compatibility

`PayloadVersion` versions the signed encoding independently from HTTP paths. A future Subxt provider replaces mock call construction, signing-payload preparation, submission, and reconciliation. Browser authentication and domain use cases remain unchanged.

## Data Ownership

Owning bounded context: custody, implemented initially by the wallet service.

Custody owns wallet records, encrypted seeds, signing jobs, leases, signatures, and custody audit records. The adapter owns principals, sessions, authorization, provider-operation status, and browser read models. The mock owns contract state, provider nonces, receipts, and durable provider events. Services exchange identifiers and versioned contracts and never access another service's tables.

Signing keys and their history are retained while they identify provider activity. Production retention of ordinary payloads and audit records has not yet been set.

## Consistency and Idempotency

Wallet provisioning is idempotent by `PrincipalId`. Signing-job creation is idempotent by `OperationId` and immutable payload hash. The mock checks both `OperationId` and the next expected account nonce. An exact duplicate returns its previous receipt; a conflicting duplicate fails. Mock verification and execution use one transaction.

No database transaction spans services. The provider worker reconciles durable states through idempotent APIs. A signed call is stored before submission so a transport retry uses identical bytes.

## Security and Privacy

Authentication: classic login and opaque adapter session, as defined by the identity specification.

Authorization: the adapter authorizes the application action; custody verifies caller identity, wallet state, operation immutability, and signing policy.

Sensitive data: private seeds and the master key are restricted secrets. Signing payloads, signatures, account identifiers, and audit records are confidential operational data unless a later contract marks a field public.

Abuse cases and controls are defined in `threat-model.md`. Audit events are secret-free, structured, and correlated by `OperationId`.

## Performance and Algorithmic Constraints

Expected proof-of-concept load is low and has no numeric service-level objective. Work is serialized per wallet and may run concurrently across wallets.

- Maximum signable payload: 256 KiB.
- Maximum pending signing jobs: 32 per wallet and 10,000 per custody instance.
- Signed mock-call lifetime: 5 minutes.
- Worker lease: 30 seconds.
- Maximum attempts per stage: 5.
- Retry delay: exponential from 1 second, capped at 30 seconds.
- Internal request timeout: 5 seconds.

Provider-specific limits may be stricter. No benchmark is required.

## Observability

Logs contain `operation_id`, `wallet_id`, stage, attempt, and stable result code; they exclude signed payload bytes and secrets. Metrics cover queue depth, oldest pending age, lease recovery, signing outcomes, submission outcomes, and retries. Traces propagate `operation_id` across service calls. Production alert thresholds are outside the local proof of concept.

## Migration and Recovery

Deployment order: custody database and service, provider worker integration, mock signature enforcement, then adapter mutations.

There is no compatibility requirement for unsigned mock calls after cutover. Mock data is disposable and is never migrated to production.

Rotating the encryption master key re-encrypts existing seeds under a new recorded version without changing wallet accounts. A signing-seed change creates a different account and requires a separate approved migration design.

Automated backup and disaster recovery are outside this proof of concept. They are mandatory before wallets control real assets.

## Test Strategy

Unit: wallet lifecycle, type-state transitions, payload hashing, expiration, and result-code mapping.

Property: changing any signed field invalidates verification; arbitrary duplicate sequences never apply an operation more than once.

Integration: encrypted provisioning, durable signing, lease recovery, per-wallet serialization, mock verification, atomic commits, and redacted telemetry.

Contract: SCALE fixtures and internal HTTP request/response fixtures are shared between producers and consumers.

Gherkin acceptance: every externally observable rule is represented in `acceptance.feature`.

End-to-end: a classic principal performs an authorized action, the backend signs it, the mock applies it, and the adapter emits the resulting SSE notification.

Performance: no benchmark; a deterministic concurrency test proves serialization per wallet and progress across wallets.

Security: tampering, replay, unauthorized calls, inactive wallets, ciphertext swapping, missing secrets, and log redaction.

## Open Questions

| Question | Owner | Blocking | Resolution |
|---|---|---:|---|
| Are the proposed local operational bounds acceptable? | Architecture | No | Local implementation approved 2026-09-08; these are transport safeguards, not configurable matching benchmarks |
| How long should signing payloads and audit records be retained in production? | Product and security | No for POC | Required before production use |
| Is XChaCha20-Poly1305 through `chacha20poly1305` 0.11 acceptable? | Security | No | Local custody implementation approved 2026-09-08; verify dependency metadata and tests |

## Approval

Product: Approved 2026-09-08 following review and explicit instruction to implement.

Architecture: Accepted review corrections and ADR-0001.

Security: Local POC only; executable verification remains required before handoff.

Data: Separate custody, adapter, and transactional mock ownership.

Approval date: 2026-09-08.
