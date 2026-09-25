# Specification Status

State: APPROVED

Owner: Kunveno product owner

Legacy source commit: `d408641`

Integrated implementation commit: `3ab2b58` on `feat/rust-rest-poc`.

Last updated: 2026-09-24

## Blocking Questions

- None for local POC implementation. The product owner accepted the review corrections and instructed implementation on 2026-09-08. Production key management and real assets remain excluded.

## Decisions Since Last Review

- Full custody; no browser export or second signing prompt.
- One `sr25519` `AccountId32` wallet per classic principal.
- Credentials are independent from custodial wallet keys.
- Dedicated custody service and SQLite database with a runtime master key.
- Proposed XChaCha20-Poly1305 seed encryption with 24-byte nonces and a 32-byte master key.
- Durable signing jobs, polling, and one in-flight operation per wallet.
- Custody signs; the provider gateway submits.
- Versioned SCALE envelope for mock calls.
- Atomic nonce, state, receipt, and event commit in the mock.
- No browser-facing arbitrary-signing endpoint.
- Wallet and operation state machines use constrained transitions.

## Active Tasks

| Task | Owner | Branch | Worktree | State | Evidence |
|---|---|---|---|---|---|
| TASK-000 through TASK-004, TASK-006 | Root integrator and implementation agents | `feat/rust-rest-poc` | Main repository | Implementation integrated; dependency policy deferred from backend closeout | `../../progress/handoffs/REST-POC-foundation.md` |
| TASK-005 | Independent verifier | `feat/backend-port-close` | Read-only integration branch | Closed for local mock-backed POC | `../../progress/handoffs/SPEC-0001-verification.md` |

## Verification and remaining work

Custody, adapter and mock implementations are integrated. Recorded evidence includes
9 custody tests, 46 workspace tests, and signed real-service flows with both memory
and SQLite, including lost-response recovery and SSE. See the integration handoff.
The six temporary task worktrees have been removed after confirming their commits
were integrated; no implementation remains isolated in them.

On 2026-09-14, `cargo-deny` 0.20.2 returned exit 5:
advisories and licenses failed; bans and sources passed. CC0-1.0 and BSL-1.0
are absent from the license allowlist. Leptos transitives `paste` and
`proc-macro-error2` have unmaintained advisories RUSTSEC-2024-0436 and
RUSTSEC-2026-0173. No exception has been applied.

TASK-005 has independent acceptance/security evidence for the local mock-backed
POC. Earlier integration checks alone did not establish that completion.
Requirement approval remains valid; no production security approval is claimed.

Independent read-only evidence review on 2026-09-14 found two specific closure gaps:

- The acceptance scenario for secret markers in diagnostics has no demonstrated
  end-to-end log scan. Existing checks cover selected error bodies and metrics;
  captured service stdout/stderr is not scanned by `scripts/poc-e2e.py`.
- Encryption master-key rotation needs scope reconciliation: the spec's recovery
  section describes re-encryption preserving accounts and the threat model calls
  for rotation checks, while `services/wallet/README.md` explicitly excludes its
  implementation pending a recovery design. This is distinct from signing-seed
  replacement. The discrepancy is unresolved, not evidence of a security defect.

This evidence review did not rerun acceptance tests and does not close TASK-005.

Update: the existing signed E2E passed again for memory and SQLite on 2026-09-15.
Its scope is documented in `../../docs/project/porting-coverage.md`. HSM-backed
envelope encryption and gradual rotation have been discussed, not implemented or
finalized. The owner accepted deferring the two Leptos maintenance advisories
and removing Leptos and cargo-deny from this POC's backend acceptance gate. The
dependency policy has not passed; this is not a production security sign-off.

2026-09-24 update: the approved backend closeout defers HSM-backed encryption-key
rotation until before real assets. `spec.md`, `data-model.md` and
`threat-model.md` now state this explicitly; the earlier discrepancy above is
historical, not an open POC implementation requirement. The revised signed E2E
passed all eight memory/SQLite scenarios with adapter PostgreSQL and scans
bounded service logs for generated secret markers. The later independent
`../../progress/handoffs/SPEC-0001-verification.md` report closes TASK-005
for the local POC only.

## Approval Record

Approver: Kunveno product owner

Date: 2026-09-08

Scope approved: Local full-custody implementation with the accepted review
corrections, REST boundary, and XChaCha20-Poly1305 stored format. Independent
backend security verification remains open. The deferred dependency-policy gate
must not be described as a production approval.
