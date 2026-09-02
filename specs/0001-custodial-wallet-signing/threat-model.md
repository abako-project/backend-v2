# Feature Threat Model

## Assets

- Custodial `sr25519` seeds.
- Encryption master keys.
- Service authentication tokens.
- Authority of human and system wallets.
- Signed provider calls and operation receipts.
- Principal-to-wallet bindings.
- Custody audit evidence.

## Actors and Adversaries

- Authorized classic principals.
- Adapter, provider, custody, and mock workloads.
- An unauthenticated network caller.
- A principal attempting an unauthorized operation.
- An attacker with a copied custody database.
- An attacker controlling an internal service credential.
- An attacker controlling the custody process or master key.
- A dependency or build-chain attacker.

## Entry Points

- Browser-facing adapter mutations.
- Internal signing-job creation and status endpoints.
- Mock signed-call endpoint.
- Custody and mock SQLite files.
- Runtime secret injection.
- Logs, traces, metrics, crash reports, and backups.

## Trust Boundaries

- Browser to adapter.
- Adapter/provider workload to custody.
- Provider gateway to mock or future blockchain.
- Custody process to its database and runtime secret provider.
- Mock process to its database.

## Data Classification

| Data | Classification | Allowed locations |
|---|---|---|
| Private seed | Restricted secret | Custody memory briefly; authenticated ciphertext in custody SQLite |
| Encryption master key | Restricted secret | Runtime secret provider and custody memory only |
| Service token | Restricted secret | Authorized workload and custody memory only |
| Signable payload and signature | Confidential operational data | Provider and custody durable state; provider transport |
| Account ID and receipt | Operational data; potentially public later | Adapter read model, custody metadata, provider state |
| Audit identifiers and hashes | Confidential operational data | Owning service audit store and redacted telemetry |

## Threats

| ID | Threat | Preconditions | Impact | Control | Residual risk |
|---|---|---|---|---|---|
| THR-001 | Custody database theft reveals seeds | Attacker copies SQLite | Wallet takeover | Authenticated encryption; master key stored separately | A leaked master key defeats the control |
| THR-002 | Ciphertext is swapped between wallet rows | Database write access | Wrong principal signs | Wallet, principal, scheme, and key version bound as associated data | Full process compromise can still sign |
| THR-003 | Forged internal request creates a signature | Internal endpoint reachable | Unauthorized operation | Internal network, service authentication, immutable job, audit | A stolen service credential retains its authority |
| THR-004 | Authorized service acts as confused deputy | Provider compromised | Unintended signature | Adapter authorization, versioned call, payload hash, wallet check, audit | Custody cannot infer every domain rule from opaque chain bytes |
| THR-005 | Signed call is changed in transit | Network attacker or faulty gateway | Wrong transition | Signature covers version, origin, target, message, nonce, expiration, and payload | Denial of service remains possible |
| THR-006 | Signed call is replayed | Attacker obtains signed bytes | Duplicate transition | Operation receipt, strict account nonce, atomic execution | Receipt must remain available |
| THR-007 | Concurrent jobs reuse a nonce | Multiple workers | Rejection or unintended ordering | One in-flight operation per wallet and conditional leases | Throughput per wallet is intentionally limited |
| THR-008 | Worker crashes after signing | Process or host failure | Duplicate signing or stranded work | Durable payload, expiring lease, conditional completion, idempotency | Signature may be recomputed before completion is stored |
| THR-009 | Secret appears in telemetry | Debugging or error path | Credential disclosure | Secret wrappers, redaction, negative tests, no raw payload logging | Memory dumps are not protected in the POC |
| THR-010 | Weak randomness creates predictable wallets | Broken RNG environment | Wallet takeover | Operating-system CSPRNG and failure on RNG error | Compromised host RNG is outside POC protection |
| THR-011 | Dependency compromise leaks keys | Malicious crate or artifact | Wallet takeover | Dependency review, lockfile, supply-chain gates, minimal features | No dependency chain is risk-free |
| THR-012 | Master key or custody process is compromised | Host or secret-store compromise | All active wallets can sign | Suspension, audit, incident procedure; KMS/HSM required for production | POC cannot protect keys from full process compromise |
| THR-013 | Browser supplies arbitrary signable bytes | Public raw-signing endpoint exists | Custody signs an unrelated transaction | No browser signing endpoint; only authorized domain actions create jobs | A compromised adapter remains trusted |

## Abuse Cases

- A caller reuses another principal's wallet ID.
- A caller substitutes a different contract instance or payload.
- A caller resubmits a valid signature after expiration.
- A caller floods one wallet with jobs to block later work.
- A caller uses one operation ID with several payloads.
- An operator enables debug output that formats a seed or token.
- A database administrator swaps encrypted seed blobs.
- A browser attempts to submit an arbitrary extrinsic for signing.

## Security Tests

- Reject missing, invalid, and unauthorized service credentials.
- Reject wallet/principal mismatches and inactive wallets.
- Reject altered version, origin, target, message, nonce, expiration, or payload.
- Reject ciphertext or associated-data substitution.
- Prove exact duplicates have one state effect and conflicting duplicates have none.
- Prove expired leases recover without two successful completions.
- Scan responses and telemetry for seeded secret markers.
- Fail startup when master-key configuration or secure randomness is unavailable.
- Verify account IDs after encryption-key rotation.
- Prove browser contracts expose no arbitrary-signing operation.

## Approval and Accepted Risk

The proof of concept accepts host-memory exposure and compromise of all wallets when the custody process or master key is fully controlled. It must not control real assets. Production use requires separate approval covering KMS or HSM protection, workload identity or mutual TLS, backup and recovery, monitoring, incident response, retention, and wallet-compromise migration.
