# SPEC-0005: Milestone rejection and public dispute opening

Status: APPROVED — implemented and verified on the mock stack.

Updated: 2026-09-23

## Goal and authority

Deliver a signed, end-to-end mock flow from milestone submission to rejection,
public dispute opening, one counterparty response and a frozen project.
Resolution is outside this PoC. An E2E ending in Open is the requested complete
flow, not a claim that adjudication is implemented.

This consolidates the owner's latest confirmations following Disputas.md.
It replaces the earlier draft's channel, snapshots, timeout and extensions.
Once approved, it amends SPEC-0003 MILE-001, TASK-002 and MONEY-003A for milestone
disputes. Planning disputes and reputation calculations retain their rules.

## Ownership

The provider/mock owns business state, signed-origin authorization, atomic
effects, receipts and domain events. The future blockchain owns the same truth.
The adapter owns sessions, pending transport operations and notification read
state. Custody signs commands; both frontends use the same REST/SSE contract.
The adapter does not store a second dispute database.

## SUB-001: Submit an identifiable delivery

Only the assigned coordinator may submit completion for a non-cancelled,
non-disputed project and a milestone in InProgress or ChangesRequested.
The request contains the deliverable reference and existing per-worker ratings.
The provider generates a submission ID and an increasing version within the
milestone, and records its author and Unix timestamp.

The submission starts PendingReview; the milestone becomes CompletionRequested.
Previous submissions/reviews remain recorded. Submission changes no payments,
accumulated reputation or reservations. Every assigned worker must still be
rated exactly once, as required by the existing completion operation.

## SUB-002: Review the current submission

Only the project client may accept or reject the current PendingReview
submission. Both commands identify its submission ID. A stale ID cannot review
a newer delivery, even when its command was queued or signed earlier.

Acceptance marks the submission Accepted and milestone Completed, with the
existing atomic payouts and reputation calculation. It uses that submission's
worker ratings. This does not introduce new scoring rules.

## REJ-001: Reject with a reason reference

Rejection requires a valid public evidence reference. It atomically marks the
submission Rejected with reason, client author and timestamp, sets the milestone
to ChangesRequested and emits MilestoneCompletionRejected.
It creates no dispute and changes no payment, score or reservation.
The coordinator does not need to accept the rejection.

## REJ-002: Resubmission replaces dispute eligibility

A new submission returns the milestone to CompletionRequested. An earlier
rejection stays in history but no longer authorizes opening. A later rejection
of the new submission enables opening against that new submission only.

## REF-001: Public HTTPS URL

Amended with owner approval on 2026-10-09: deliverable, rejection reason,
opening argument and counterparty response contain only url. SHA-256 is no
longer required or validated as content evidence. New records omit it; legacy
records retain it only to preserve signed-command JSON/SCALE round trips.

<!-- Previous requirement suspended: each reference included a SHA-256 digest
committing to exact artifact bytes. Do not restore without owner approval. -->

The adapter/provider validate the HTTPS URL, sign and store the reference.
Neither fetches the URL, verifies remote content or guarantees availability.
External hosts can change/remove content. Freezing project state does not
freeze a website. Hosting and long-term retention remain outside this PoC.

Older JSON records remain readable. The reserved SCALE digest slot is retained
for already signed operations; it is not a content requirement. Internal
signing-payload hashes, signatures and transaction identifiers are unchanged.

## DSP-001: Open only against a current rejection

The project client or assigned coordinator may open when the project is not
cancelled and has no active dispute, the milestone is ChangesRequested, the
supplied submission ID is its current Rejected submission, and the opening
evidence reference is valid.

InProgress, CompletionRequested and Completed do not permit opening.
Workers, unrelated accounts and system authority have no bypass.

## DSP-002: Create and freeze atomically

Opening generates a dispute ID and records project, milestone, rejected
submission, opener, counterparty, opening evidence and timestamp. It records
the existing proposal revision and the committed event cursor immediately
before opening. A proposal revision alone does not version every task mutation.

The same provider transaction creates the Open case, sets project
active_dispute_id, sets the affected milestone to Disputed and emits one
DisputeOpened event with its ID. Failure commits no partial business effects.

No full project/task snapshot is stored. The frozen records remain the opening
context. This preserves state at opening, not task contents at an earlier
delivery. Today's metadata-only events cannot reconstruct arbitrary past state.

## DSP-003: Freeze the whole project and preserve funds

While active_dispute_id is present, reject every project mutation: proposal
editing/deletion/submission/approval, task creation/edit/progress, completion
submission/acceptance/rejection, planning changes/disputes and cancellation.
Another opening fails even for another milestone.

Opening and denied mutations preserve balances, planning/execution escrow,
assignments, accumulated ratings and calendar reservations. Existing payments
are not reversed. Other milestones retain their status but inherit the project
freeze. No funds are paid, refunded or unreserved.

Reads, notifications and the single formal counterparty response remain
available. The response changes only the dispute. Other projects, catalog and
personal calendar edits keep their existing rules and cannot remove this
project's reservations.

## DSP-004: One immutable counterparty response

The other project principal may append one evidence reference with verified
author and timestamp. The opener, workers, administrator and unrelated accounts
cannot respond on its behalf. Opening and response references cannot be
edited/deleted. A second operation ID cannot overwrite the response.
The dispute remains Open after responding.

## DSP-005: Public case without a communication channel

The dispute is public immediately; Public is not a lifecycle state.
GET /api/disputes/{disputeId} requires no login. Its explicit projection contains
case identities, state, parties, timestamps, references, rejected submission/
review, context revision/cursor and the affected frozen milestone definition
and task storage. Related submission/review metadata comes from retained records.

Never return sessions, keys, credentials, signed payloads, receipts, notification
recipients/read state, unrelated projects or personal calendar reservations.
Published accounts and references are public; adapter login cannot conceal
onchain data. No chat, channel identity or additional argument loop is created.

## DSP-006: Durable events, replay and concurrency

Provider events feed the existing adapter ingestor and authenticated SSE.
Client and coordinator observe opening/response after reconnecting. Public
reads do not change notification read state.

Exact replays return the original receipt before current-state guards.
Different operation IDs obey uniqueness and response limits. Concurrent
openings serialize: one succeeds, the other sees the active dispute.
Business failures leave no partial effects; existing failed receipt/nonce
semantics remain intact.

## DSP-007: No resolution or unlock

The case stays Open and the project frozen. No user, administrator or
system-origin operation can resolve/unlock it. DAO authority, decisions,
escrow disposition and post-resolution states require a later specification.
Resetting disposable tests is not a resolution mechanism.
Timeout, due_at extensions, arbitration, refunds and penalties are excluded.

## Public REST contract

| Method and route | Input / result |
|---|---|
| POST /api/projects/{projectId}/milestones/{milestoneId}/completion-submissions | Deliverable reference and worker ratings; operation reference |
| POST /api/projects/{projectId}/milestones/{milestoneId}/request-completion | Compatibility alias with the same input and behavior |
| POST /api/completion-submissions/{submissionId}/rejection | Reason reference; operation reference |
| POST /api/projects/{projectId}/milestones/{milestoneId}/accept-completion | Current submission ID and existing client ratings; operation reference |
| POST /api/disputes | Project, milestone, rejected submission IDs and evidence; operation reference |
| POST /api/disputes/{disputeId}/response | Evidence reference; operation reference |
| GET /api/disputes/{disputeId} | Public case projection |

Writes retain session/CSRF/idempotency protections, return HTTP 202 and expose
final outcome through /api/operations/{operationId}. Created IDs come from
provider receipts. Signed commands bind resolved resource targets; provider
rechecks every relationship. Bodies cannot supply trusted authors/timestamps.

## Implemented technical decisions

The existing request-completion and accept-completion routes remain available.
completion-submissions is an alias for versioned delivery submission. The old
precondition-free milestone dispute route and DisputeMilestone command were
removed, so every milestone dispute requires a current rejection.

Signed payload version 2 covers the changed command encoding. Memory and SQLite
use disposable state in this PoC; incompatible retained state is never deleted
automatically. PostgreSQL remains separate work across provider, adapter and
custody.

Observed verification is recorded in status.md and the DSP-07 handoff. Resolution,
unlock, DAO adjudication, timeout and due_at extensions remain outside this
approved specification.
