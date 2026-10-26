# Proposal presentation and scoped review API

See the [approved requirements](../../specs/0007-proposal-review-presentation/spec.md). These additions persist descriptive information in adapter-api/PostgreSQL; provider commands, assignments and payments are unchanged.

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

Historical validation and deployment evidence remains in
[Git history](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/project/proposal-review-api.md).
