# Technical design: dispute opening

Status: DRAFT

## Current baseline

The current signed `DisputeMilestone` command accepts either `InProgress` or
`CompletionRequested`, changes the milestone to `Disputed`, sets `frozen`, stores
one private `RecordedReason` and emits `MilestoneDisputed`. It has no rejection
precondition. `RecordedReason` is serialized in provider state but omitted from
the provider snapshot and all public queries.

There is no reject-completion command, dispute aggregate, dispute ID, evidence
snapshot, argument append, communication channel or public dispute view. The
adapter route only submits the existing command. Provider events notify project
participants, but an event contains no case record or argument.

## Decision summary

Add a cohesive dispute module inside the transactional provider rather than a
new microservice. The provider remains the business source of truth. The adapter
maps authenticated REST actions to closed signed commands and exposes explicit
read DTOs. Resolution and DAO integration stay absent.

Keep dispute logic out of the already large provider dispatch function: the
domain module owns validation and state transitions; the composition layer only
routes commands and commits returned effects. New tests live in dedicated
`tests.rs` files.

## Command flows

### Reject completion

1. Adapter authenticates the client and validates CSRF/origin.
2. Custody signs `RejectMilestoneCompletion` for that client.
3. Provider verifies signature, project ownership, review state and reason.
4. Provider appends rejection and event without paying or scoring.
5. Adapter exposes the durable receipt and notification through existing
   operation polling and SSE.

### Open dispute

1. Adapter authenticates the client or assigned coordinator.
2. Custody signs the closed command with project, milestone and argument.
3. Provider rechecks party authorization and finds a qualifying rejection.
4. Provider captures evidence at one event-cursor boundary.
5. Provider creates case, opening argument and channel, freezes the milestone and
   commits all related events in one storage transaction.
6. Adapter returns an operation reference; case IDs come only from the receipt.

### Add counterparty argument

1. Adapter authenticates the counterparty and signs the append command.
2. Provider rechecks the dispute's stored counterparty and validates content.
3. Provider appends one argument and event atomically.

Read routes never accept an actor in the request body. Private and public views
are distinct projections of provider-owned state.

## Contracts and compatibility

The contract task first freezes:

- the completion-request reference and rejection DTO;
- dispute status semantics;
- signed commands, views, events and stable errors;
- text and collection bounds;
- public redaction and pagination;
- payload compatibility for replacing the permissive command.

Adding enum variants affects SCALE wire compatibility. Do not rely on source
ordering accidentally remaining compatible. Either bump the payload contract or
provide a deliberate transition that rejects the old precondition-free path.

The existing endpoint may be retired or mapped to the new open command only after
compatibility review. It must not remain a bypass around the rejection rule.

## Evidence capture

Evidence capture selects only the target project and milestone plus related
events through a fixed cursor. It clones the recorded data into a versioned
snapshot. Future mutations append new records and cannot rewrite the snapshot.

No current contract stores deliverable bytes, versions or explicit acceptance
criteria. The implementation records those fields as unavailable rather than
accepting retrospective user content as historical evidence. Q-008 decides how
future completion submissions become identifiable.

Content hashing is not part of this PoC. The mock retains its versioned snapshot
without an on-chain/off-chain split. A future real-chain specification must
freeze canonical encoding and the integrity commitment; an ad hoc JSON hash
would not be a stable chain commitment.

## Public view and channel

The public endpoint uses a purpose-built allowlist. It never serializes internal
provider state, signed payloads, receipts, session data or notification read
state. Implementation waits for Q-003.

The provider can atomically create channel identity with the dispute. Actual
messages require Q-004; a channel ID alone must not be presented as a functioning
chat. A future realtime transport may deliver channel updates, while durable
message authority remains in the provider or another explicitly approved owner.

## Failure, retry and concurrency

The existing provider clone/apply/commit transaction remains the serialization
boundary. Concurrent openings observe committed rejection/dispute state in order.
The Q-006 uniqueness decision determines whether the second valid opening fails
or creates another case.

Existing operation IDs and receipts handle lost responses. Replaying an operation
returns its original receipt. A different operation ID is a new business request
and must obey the approved uniqueness rule.

Argument/event pages and evidence size need bounded limits before approval. No
unbounded channel history is returned in one response.

## Security controls

- Adapter and provider enforce authorization independently.
- The signed origin determines author and party; body-supplied authors are absent.
- Rejection and argument text is length-bounded and rendered as text by clients.
- Internal evidence and public evidence use separate DTOs.
- Events and logs omit free-text arguments.
- Normal milestone completion rejects a disputed/frozen milestone.

## Observability

Record command outcome, dispute ID, project/milestone IDs, actor account, event
cursors and latency. Do not log reasons, arguments or evidence bodies. Track
rejection, opening, append, authorization denial, validation failure and rollback
counters without using them to infer a resolution.

## Validation plan

1. Resolve the blocking questions in `status.md` and change state to `APPROVED`.
2. Freeze contracts and payload compatibility.
3. Run focused contract tests.
4. Run identical provider transition suites for memory and SQLite.
5. Run adapter authorization/redaction/SSE tests.
6. Run a signed E2E with rejection, optional opening, response and frozen funds.
7. Run the workspace quality gates and independent requirement trace review.
