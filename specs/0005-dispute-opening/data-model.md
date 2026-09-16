# Data model: dispute opening

Status: DRAFT

## Ownership

The provider dispute context owns all records below. The adapter may project
sanitized reads and notification state but does not persist a competing dispute.
The same logical aggregate is stored by the memory and SQLite mock backends.

## Proposed domain records

Names are descriptive until contract approval.

### Milestone rejection

| Field | Meaning |
|---|---|
| `rejection_id` | Provider-generated opaque ID |
| `project_id` / `milestone_id` | Existing aggregate references |
| `completion_request_ref` | Exact completion submission rejected; representation depends on Q-008 |
| `rejected_by` | Verified client account |
| `reason` | Validated non-blank untrusted text |
| `occurred_at` | Provider Unix timestamp |
| `event_cursor` | Durable event-stream position |

Rejections are inserted, never updated or deleted.

### Dispute

| Field | Meaning |
|---|---|
| `dispute_id` | Provider-generated opaque ID |
| `project_id` / `milestone_id` | Existing aggregate references |
| `trigger_rejection_id` | Rejection that made opening eligible |
| `opened_by` / `counterparty` | The two project principals |
| `status` | Exact enum and transitions pending Q-002 |
| `opened_at` | Provider Unix timestamp |
| `opening_argument_id` | Exactly one argument owned by this dispute |
| `evidence_id` | Exactly one immutable opening snapshot |
| `channel_id` | Exactly one linked communication channel |

The uniqueness rule across milestone, rejection and active dispute awaits Q-006.

### Argument

| Field | Meaning |
|---|---|
| `argument_id` / `dispute_id` | Opaque identity and owner |
| `author` | Verified opener or counterparty account |
| `kind` | `Opening` or approved response kind |
| `content` | Validated untrusted text |
| `occurred_at` | Provider Unix timestamp |
| `event_cursor` | Durable append position |

There is no update or delete operation. Additional kinds depend on Q-007.

### Evidence snapshot

The snapshot contains a schema version, capture timestamp and terminal event
cursor plus copies of the data available at opening:

- project parties and project definition;
- approved proposal and milestone definition;
- milestone state, assignments and frozen escrow context;
- attached task storage and its tasks;
- completion requests and recorded rejections that exist at that time;
- project/milestone domain events through the terminal cursor;
- explicit markers for requested evidence that the platform does not possess.

The internal evidence may contain fields that are not public. A separately built
`DisputePublicView` applies approved field allowlists. The provider must not copy
the complete global snapshot or unrelated projects.

### Communication channel

The minimum record has `channel_id`, `dispute_id`, client, coordinator and
creation time. Message records and their relation to formal arguments are not
defined until Q-004 is answered.

## Aggregate invariants

- Every reference resolves inside the same provider state.
- Evidence belongs to one dispute and cannot be replaced.
- Every argument belongs to one dispute and preserves insertion order.
- The opening argument author equals `opened_by`.
- A response author equals `counterparty` under the current PoC scope.
- The channel participants equal the dispute client and coordinator.
- Frozen milestone value remains part of execution escrow.
- State restoration validates all references, event cursors and uniqueness rules.

## Transaction boundaries

`RejectMilestoneCompletion` commits the rejection and event together.
`OpenMilestoneDispute` commits dispute, opening argument, evidence, channel,
freeze and events together. `AddDisputeArgument` commits one append and event.
Failure leaves the pre-command state unchanged except for the existing failed
receipt/nonce behavior defined by SPEC-0003.

## Persistence and migration

The current SQLite backend stores the complete provider aggregate as JSON. The
implementation must either provide explicit defaults and validation for the new
collections or document a PoC reset requirement. No PostgreSQL schema is implied.
Before a real chain migration, freeze SCALE discriminants, payload version,
evidence encoding and any off-chain content-addressing policy.

## Retention and privacy

The source document requires immutable evidence and arguments, so deletion and
retention cannot be inferred. Public redaction must not modify the internal
record. Data retention, erasure handling and evidence access need security/data
approval before production use.
