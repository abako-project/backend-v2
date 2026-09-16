# DISPUTE-PLAN handoff

## Outcome

Created the DRAFT implementation specification for milestone rejection and
dispute opening. No production code, public contract or runtime state changed.

## Source evidence reviewed

- `Disputas.md` and the supplied meeting minutes.
- SPEC-0003 milestone, task, money, authorization and storage rules.
- SPEC-0004 governance boundary.
- ADR-0001 service and frontend boundary.
- Current generated contracts, provider transition, provider tests, adapter
  authorization and REST mapping through the indexed code graph.

The graph reported no recorded coverage issue for the contract, provider domain,
provider transaction tests, adapter HTTP or adapter authorization test files.
Direct source reads were limited to exact contract/data-model gaps.

## Current gap

The current command opens from `InProgress` or `CompletionRequested` without a
rejection and only stores a private reason plus milestone freeze. It has no case
entity, evidence snapshot, append-only argument stream, channel or public view.

## Artifacts

- `specs/0005-dispute-opening/spec.md`
- `specs/0005-dispute-opening/data-model.md`
- `specs/0005-dispute-opening/design.md`
- `specs/0005-dispute-opening/acceptance.feature`
- `specs/0005-dispute-opening/tasks.md`
- `specs/0005-dispute-opening/status.md`

## Decisions preserved

- A rejection needs a reason and does not automatically create a dispute.
- The client or assigned coordinator may open after the first rejection.
- Opening needs an argument and captures available history immutably.
- The counterparty may append an argument.
- Opening creates a linked communication channel and public sanitized view.
- Unreleased funds freeze; no resolution or fund disposition exists in this PoC.

## Blockers

Implementation is blocked on the decisions listed in `status.md`:
post-rejection state, status transitions, public redaction, channel semantics,
case multiplicity, later arguments and completion-submission identity.
The real-chain evidence boundary and canonical hash are deferred beyond this PoC.

## Verification

Documentation checks, Gherkin structure, path-safety scan and `git diff --check`
are required before commit. Runtime tests are not applicable because this branch
contains specification files only.
