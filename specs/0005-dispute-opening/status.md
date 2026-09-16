# Specification status

State: DRAFT

Last updated: 2026-09-16

## Implemented baseline

SPEC-0003 currently authorizes the project client and assigned coordinator to
mark planning or milestone work as disputed and freeze unreleased funds. The
milestone command accepts `InProgress` or `CompletionRequested`, stores one
private reason, changes the milestone to `Disputed` and emits an event.

That baseline is not the dispute-opening system described here. It lacks the
required prior rejection, a dispute entity and ID, immutable evidence, append-only
counterparty arguments, a communication channel and a public sanitized view.

## Blocking product decisions

- Q-001: milestone state and resubmission after rejection.
- Q-002: meaning and triggers of `Open`, `Public` and
  `PendingDaoResolution`.
- Q-003: public audience and field-level redaction.
- Q-004: relationship between channel messages and formal arguments.
- Q-006: multiplicity and conflict rules for disputes.
- Q-007: number and authorship of later arguments.
- Q-008: identity and versioning of the completion submission being rejected.

## Scope held outside the PoC

Resolution, DAO voting, judge selection, escrow disposition, penalties and
compensation remain out of scope. SPEC-0004 governance remarks do not confer any
of those powers. The on-chain/off-chain evidence boundary and canonical hash are
also deferred; the PoC keeps a versioned snapshot in the transactional mock.

## Approval record

Product, architecture, security and data approval: pending.

No development worktree from `tasks.md` should start until this state changes to
`APPROVED`.
