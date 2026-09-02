# Specification Status

State: DRAFT

Owner: Kunveno product owner

Current source commit: `d408641`

Last updated: 2026-09-02

## Blocking Questions

- Approve the proposed local bounds recorded in `spec.md`.
- Approve XChaCha20-Poly1305 through `chacha20poly1305` 0.11 and its versioned stored format.

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
| TASK-001 through TASK-006 | Unassigned | Not created | Not created | Blocked by DRAFT specification | `tasks.md` |

## Approval Record

Approver: Kunveno product owner

Date: Individual decisions approved through 2026-09-02

Scope approved: Conversation design; consolidated written specification pending review.
