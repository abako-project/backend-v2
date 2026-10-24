# R3a: prepare task consumers before changing ownership

This is a compatible reader milestone within approved SPEC-0015. The owner
selected frontend compatibility first on 2026-10-24. Task execution,
permissions, signing, provider state, notification ingestion and emitted payloads
remain unchanged. It does not enable adapter-owned task writes.

## Why separate this step

Today the frontend considers a finalized operation without a provider receipt
pending. Notifications require a provider event, including its provider cursor.
Adapter-only edits cannot truthfully supply either. First ship consumers that can
read a task result and an explicitly adapter-owned task notification. Switching
task ownership requires confirmation that the independent frontend has adopted
these forms; elapsed time or this implementation does not establish readiness.

## Consumer contract

- Append dormant `Queued` to the operation status vocabulary. It means durably
  accepted API work waiting for execution, including edit holds, and implies no
  signing or provider submission. Existing status SCALE indices stay unchanged.
  Current writers and their PostgreSQL status constraint are unchanged.

- `OperationView` accepts an optional `taskResult`, omitted for all current
  operations. The result contains the execution outcome, task storage ID and
  completion time. It contains no nonce, signature or provider event cursor.
  Finalized success requires exactly one known result: `receipt` or `taskResult`.
  Missing or conflicting results remain unresolved; clients retain the operation
  reference instead of submitting another business action.
- `NotificationView.event` accepts the existing provider event or a task event
  with required `source: "AdapterTask"`. A task event identifies its provider
  generation, operation, project, task storage, task event kind, actor, recipients
  and time. The generation scopes entity IDs; it is not a claim of onchain
  execution. Task events contain no provider cursor.
- Existing provider event JSON and operation JSON without a task result serialize
  exactly as before. Both notification forms retain the same outer notification
  ID, read state, actor and title fields. SSE delivery still resumes/deduplicates
  by `notificationId`, never a task/provider event cursor.

R3a adds no table, task writer, authority flag, queue, workflow framework or
contract artifact. The built-in frontend learns both forms; examples document
what the independent frontend needs to accept. This is consumer preparation,
not proof that an old adapter can execute future queued API tasks.

## R3b cutover requirements

The task writer still needs the approved transactional allocator, authorization,
project edit hold, immutable dispute snapshot, durable recovery and view/recipient
composition. Future API task notifications must allocate IDs under the same
short PostgreSQL transaction lock used by provider ingestion (621008), holding it
through commit. A separate task cursor would let SSE skip a delayed commit.
Task results and notifications must commit with the task change. Provider receipt
and signing fields stay empty for API-only operations.

Before enabling that writer, verify independent frontend readiness, both route
forms, known-worker validation, task-only visibility/recipients, all approved
freeze races, idempotency/restart, exact money/reputation invariance, and a concrete
paired current-state rollback procedure. Do not claim that R3a alone meets R3's
migration, runtime or rollback acceptance criteria.

## R3a checks

Check exact existing wire examples and rejected mixed/unknown notification forms;
exercise frontend success/failure/pending interpretation for each result source;
run existing adapter notification privacy/replay/read-state tests and the app
verification gate. No canonical build is needed because canonical code and signed
commands are unchanged. Record actual results and the external frontend limit in
the milestone handoff and draft PR.

## R3b1: persist and read future task history

This next compatible milestone fills the persistence gap in R3a. It introduces
no task writer or authority switch. Current task routes still enqueue signed
provider commands and return the existing responses. No canonical artifact,
public DTO or dependency changes.

The existing operations table gains `task_operation` (default false) and
`task_result_json`. API task records retain their generation, admit only
Queued/Finalized/Rejected/Expired and forbid signing payloads, signed calls,
provider receipts, possible submission and catalog-lane ownership. Finalized
requires a task result only for API task rows; provider finality keeps its old
semantics. Polling reads validate retained task command context and the result's
storage ID without consulting current provider state.

Notifications retain their existing sequence and provider uniqueness key. An API
operation key and nullable provider cursor identify exactly one source. A task
notification binds to a retained successful finalized task operation: generation,
operation, project, storage, actor, kind and time must match, and the recipient
must occur in the captured audience. Historical provider events do not require
an adapter operation row. Optional title enrichment retains its old availability
and visibility rules. Bounded pages with potentially large retained commands
are decoded away from the Tokio reactor.

The provider claimer excludes API task candidates; construction, execution and
save also defend that boundary. Earlier same-wallet API work remains a FIFO
blocker. This reader cannot run, expire or reject future queued API work. The
supported future downgrade must drain/reconcile that work first and retain its
actual outcomes. Do not select R3a as a rollback target for task-owned history:
it cannot read durable task results and its provider claimer lacks this guard.

This is still not a full R3 rollback target. The later paired converter must
restore actual current tasks and their allocator into the predecessor core,
preserve API result/notification history, and prove further public work. Seeded
future-history tests prove this reader/schema seam, not API task execution or
such a conversion. No live database is migrated by the development checks.

The active writer also needs an explicit scheduling rule under the approved
snapshot/edit-hold policy: a task queued before a later same-wallet freeze must
not wait for a hold whose freeze is itself blocked by ordinary wallet FIFO.
The bridge deliberately leaves unexpected API work unresolved; it adds no
speculative bypass, scheduler or second queue to solve the active-writer race.

R3b1 acceptance covers predecessor-schema upgrade and repeat startup, owner-only
polling, both task-route replay forms without signing, mixed-source SSE resume,
explicit read state, context corruption and SQL source constraints. Existing
application gates must pass. External frontend adoption, actual task execution,
freezes, migration and current-state rollback remain R3b exit gates.
