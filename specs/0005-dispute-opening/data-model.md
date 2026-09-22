# Dispute opening: data model

Status: REVIEW REQUIRED — 2026-09-22

The provider owns these records. They are not adapter database entities.

## EvidenceReference

Validated url and sha256. Submitter commits to exact artifact bytes; provider
does not fetch or attest them. No private-text variant or upload subsystem.

## CompletionSubmission

- Provider-generated submission_id; milestone owns its collection.
- Increasing version within that milestone, starting at 1.
- Deliverable reference, verified coordinator and Unix submission timestamp.
- Existing coordinator ratings of assigned workers.
- Review enum: PendingReview; Rejected { reason, reviewed_by, reviewed_at };
  Accepted { reviewed_by, reviewed_at }.

Only review can transition once from pending to terminal. Identity, version,
deliverable, author, creation timestamp and ratings cannot be edited.
Resubmission appends a record; earlier submissions never regain eligibility.

## Milestone and project

Milestone retains submissions and adds ChangesRequested. Latest pending
submission corresponds to CompletionRequested; rejection corresponds to
ChangesRequested or the disputed milestone; acceptance corresponds to Completed.

Project stores active_dispute_id: Option<EntityId>, the authoritative freeze
guard for all attached proposals, milestones and task storages. No new project
lifecycle enum or synthetic task revision system is required.

## Dispute

- dispute_id, project_id, milestone_id, rejected_submission_id.
- status: Open.
- opened_by, counterparty, opened_at.
- evidence: EvidenceReference.
- response: Option<DisputeResponse>.
- Existing proposal revision and context_event_cursor immediately before opening.

Response contains evidence, verified author and timestamp, inserted once.
No argument vector, communication channel or resolution fields.
Without closing, a project can have at most one dispute during this PoC.

## Public projection

Join the case, rejected submission/review and affected frozen milestone definition/
task storage. Related delivery metadata comes from retained records.
No duplicate snapshot, unrelated project data or internal event recipients/
receipts. Public account IDs and evidence references are intentional.

## Invariants and restoration

Unique IDs; resolvable parent links; increasing versions; consistent review/
milestone state; coordinator submission authors; client review authors;
active link to an Open case for the same project/current rejection; response
author equals counterparty. Opening conserves funds and reservations.

Retained context is state at opening, not earlier submission task contents or
external hosted bytes. Future unlock design must address historical context
before permitting edits again.
