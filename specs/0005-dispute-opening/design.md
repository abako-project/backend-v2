# Dispute opening: technical design

Status: IMPLEMENTED — 2026-09-23

## Reuse the existing flow

adapter http::command -> operations::enqueue/authorize -> custody signing job
-> provider HTTP call -> Provider::execute -> State::execute -> project dispatch.

Provider commits domain state, receipt and event atomically. Its ordered stream
feeds adapter notifications/SSE. Keep existing retry and replay semantics.

State::apply_project_command is the central project guard: add the active-dispute
check there so all project commands are covered. Response uses an explicit
dispute operation and party check; no broad bypass of the freeze.

## Contracts

Add ordinary derived structs/enums in focused submission/dispute modules.
No new DTO-generating macros or generic state-machine framework. Evidence uses
validated private fields and constructors reused by JSON/SCALE decoding.
Runtime validation still checks signed origin, current state and ancestry.

Proposed technical bounds: absolute HTTPS URL, maximum 2048 UTF-8 bytes, no
embedded credentials or control characters; a fixed 32-byte SHA-256 value.
Fixtures use valid HTTPS references without fetching them. IPFS can use HTTPS
gateway references; no implicit downloader or additional URI scheme support.

Keep signable-payload and HTTP bounds. Public case reads respect response size
limits and fail explicitly if exceeded; never silently truncate evidence.
Any separate history reads introduced later must be paginated.

Bump the payload version for changed command encodings. Pin enum discriminants
where wire compatibility requires it; never reinterpret old persisted bytes.
Compatibility is approved: payload version 2 and disposable test state are implemented.

## Provider

Retain submission history under the milestone and disputes in a provider-owned
map; project stores its active link. Shared helpers resolve submission/dispute
targets. Provider rechecks resource ancestry independently from adapter.

Submitting appends a version. Accept/reject reference the exact current ID,
closing the stale-review race introduced by resubmission. Acceptance uses the
submission's worker ratings and existing settlement arithmetic.

Opening creates the case and project link, changes the target milestone and
emits DisputeOpened in one transaction. GET joins case, submission and frozen
target records without storing a snapshot. Metadata-only events provide a
timeline, not a complete reconstruction of earlier task contents.

Restoration validates unique IDs, increasing versions, parent/party relationships,
review state, response authorship, project/dispute links and escrow conservation.
Failed business transitions retain existing failed receipt/nonce behavior.

## Adapter and clients

Focused handlers map confirmed REST routes to closed signed commands.
Only the exact public case GET/HEAD bypasses login; never exempt the entire
/api/disputes prefix from authentication/CSRF.

Use an authenticated internal case query instead of fetching the whole provider
aggregate for public reads. Return a purpose-built DTO, never State, a receipt
or a notification object. Resolve IDs without trusting body-supplied authors.

Add MilestoneCompletionSubmitted, MilestoneCompletionRejected, DisputeOpened
and DisputeResponseAdded to existing notifications/SSE. Response remains allowed
under freeze, but cannot change the frozen project.

Adapt existing Leptos completion controls/browser fixtures to changed contracts.
Initial proof is real-service HTTP E2E; a full Figma dispute-management UI is
separate work and cannot be claimed from API tests.

## Typed errors and tracing

Use a dispute-domain error enum with stable public code mapping. Malformed
evidence is rejected as invalid_request at the JSON boundary; domain codes are:
submission_not_pending, submission_not_current,
milestone_not_changes_requested, project_disputed, active_dispute_exists,
dispute_response_forbidden, dispute_already_answered, plus existing not-found,
auth/CSRF, idempotency and payload-version errors.

HTTP validation/authorization can fail before enqueue. Later domain failures
appear through operation receipts. Document both in OpenAPI.

Use info! for committed transitions, warn! for rejected transitions and error!
for storage/internal failures, with IDs, codes and cursors. Do not log URLs,
documents, bodies, credentials or signing bytes. Initialize a subscriber in
participating executables so the E2E can actually collect the tracing output.

## Storage boundary

Proposed first delivery uses memory and SQLite with equivalent domain behavior.
PostgreSQL remains requested separate adapter/custody/provider/Compose work.
Incompatible retained state fails clearly; tests use temporary databases.
Never auto-reset user data or claim production chain/DAO compatibility.

## Verification

Contract tests cover constructors and JSON/SCALE validation/versioning. Provider
tests cover rejection/resubmission, all freeze guards, resource ancestry, one
response, races, replay and restoration, in dedicated tests.rs modules.
Adapter tests cover public GET versus protected writes and SSE replay.

E2E starts real adapter/custody/mock services and uses public REST/SSE. Fixtures
contain exact evidence bytes/digests; no real repository or remote host is needed.
Exercise acceptance and dispute branches, changed delivery versions, frozen
other milestones, conservation and idempotency. Keep four-milestone 5/3/4/2
regression passing. See acceptance.feature and tasks.md.
