# Proposal presentation and scoped review API

BE-C1/BE-C2 was approved on 2026-10-09 and is implemented locally for frontend C5–C9. Specification: `specs/0007-proposal-review-presentation/spec.md`. These additions persist descriptive information in adapter-api/PostgreSQL; provider commands, assignments and payments are unchanged.

## Contract

Under `/api/projects/{projectId}/proposals/{proposalId}`:

| Method/path | Permissions | Input/result |
| --- | --- | --- |
| GET `/presentation` | Existing project participants | Current saved presentation or null. Includes saved proposal revision and whether its business definition still matches. |
| PUT `/presentation` | Assigned coordinator, editable Draft, cookie + CSRF | `expectedProposalRevision`, `expectedPresentationRevision`, keyed milestone descriptions and nullable requested experience. 201 first/new version; 200 exact retry; 409 conflict. |
| GET `/comments?after=0` | Client/coordinator | Ordered immutable responses, maximum 20 per page and nullable `nextAfter`. |
| POST `/comments` | Client/coordinator, cookie + CSRF | `requestId`, `expectedProposalRevision`, `message`. Server supplies author, timestamp and current definition/presentation snapshot. 201 new, 200 exact retry, 409 stale/body conflict. |

Anonymous requests are 401; invisible projects are 404; known participants without the required actor permission receive 403. Frozen/cancelled projects and cancelled proposals are read-only. Provider unavailability fails with 503. No contact email is exposed.

Descriptions accept at most 2,000 UTF-8 bytes; comments 10,000 bytes, nonblank and no NUL. Maximum 100 milestones and 100 experience entries per milestone; keys must be unique and present in the real provider definition. Experience is Junior/MidLevel/Senior/null and does not alter matching.

Migration `0007_proposal_reviews.sql` adds append-only presentation versions and comments, scoped to provider instance/project/proposal. The startup migration is rerunnable. Advisory transaction locks and a unique author/request key serialize writes and exact retries. Comments retain immutable historical scope snapshots; status changes alone do not invalidate a matching presentation. The adapter does not provide a cross-service transaction or rollback: confirm provider commands before dependent descriptive writes and retry only the failed write.

## Validation and local deployment

79 workspace nextest tests passed using an isolated PostgreSQL, including permissions, CSRF, concurrent writes, stale revisions, retries, paging, restart/provider-instance isolation and unchanged provider state/payments. Two doctests, fmt, cargo check, strict Clippy, workspace build and the 79-operation OpenAPI check passed.

The final adapter release image was built and deployed in the existing `abako-auth-review` Compose stack, without resetting application data or rebuilding/restarting provider and wallet. Frontend live checks verified descriptions, requested experience, task writes, review comments and separate planning/execution payments. The disposable test PostgreSQL was removed after validation.

Existing dependency gates remain: cargo audit exits 0 with paste/proc-macro-error2 unmaintained warnings; cargo deny fails the existing CC0 license policy/advisories. Cargo.toml/Cargo.lock were not changed in this extension.

This delivery excludes the existing unrelated `.gitignore` change and local review/handoff files. No additional backend worktree was created.
