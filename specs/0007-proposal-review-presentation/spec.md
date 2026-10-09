# SPEC-0007: Proposal presentation and review comments

Status: APPROVED — product owner approved BE-C1/BE-C2 on 2026-10-09.

The adapter owns milestone descriptions, optional requested experience and scope
review comments in PostgreSQL. These fields do not change matching, capacity,
proposal business states, assignments, planning settlement or execution funding.

GET/PUT /api/projects/{projectId}/proposals/{proposalId}/presentation reads/writes
bounded metadata keyed by actual milestone/requirement keys. Only the assigned
coordinator writes editable Draft proposals. Reads follow existing project
participant visibility. Writes check expectedProposalRevision and
expectedPresentationRevision, retain immutable versions, and make exact retries
idempotent. A presentation matches the current proposal only when its saved
business definition matches; submission's status/revision change alone does not
hide unchanged descriptions. Unknown or duplicate keys are invalid.

GET/POST /api/projects/{projectId}/proposals/{proposalId}/comments is limited to
the project client/coordinator. New comments contain expectedProposalRevision,
message and a 16-byte requestId. The server supplies author, timestamp and an
immutable current proposal definition/presentation snapshot. Comments are plain
text, at most 10000 UTF-8 bytes; revisions/cursors are nonnegative. Pages contain
at most 20 records, ordered by ID. Exact retries return the same record even if
the provider revision has since changed; different bodies for the same author
and requestId conflict. Comments cannot change any provider business state.

Records are scoped to provider instance, project and proposal. No private email,
session data, arbitrary HTML or task/financial secrets are copied. Frozen or
cancelled projects and cancelled proposals are read-only. Metadata input has at
most 100 milestones, each with at most 100 experience entries. Descriptions are
at most 2000 UTF-8 bytes; experience is Junior, MidLevel or Senior, or null.

Confirm business operations before dependent descriptive writes. Preserve the
created IDs and draft when a descriptive write fails; retry only that write.
Tests cover permissions, validation, revisions, retries, concurrent writes,
provider reset, persistence and absence of provider state/payment changes.
