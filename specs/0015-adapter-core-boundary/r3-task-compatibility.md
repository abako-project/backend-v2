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

## R3b2: verified task state and retained allocator preparation

The provider exports one selected `TaskStateV1` over authenticated private HTTP.
Its generation, actual next task key, storage contexts and task data come from
one native state or one SDK `read_reference` invocation. No counter is inferred
from live IDs. No signed command, core artifact or public route changes.

`adapter-api backfill-task-state` prepares three final-purpose tables: generation
counter/provenance/counts, storage context, and task rows. The provider remains
authoritative; existing R1 composition and all public task routes are unchanged.
Storages use deterministic storage-ID order; each original task vector uses an
ordinal. Empty storages, deleted-ID gaps, authors, all u64 revisions/timestamps,
Unicode/NUL content and assignee order survive exactly. Narrative stays JSON text;
the separate assignee array comes from the same typed task and is checked on read.
The exhausted u32::MAX counter is retained; it is not an allocatable/wrapping key.

One backfill lock is acquired before fetching, preventing an older capture from
committing after a newer one. Replacement is generation-scoped and transactional;
reconstruction must equal the captured graph before commit. Identical input is
idempotent. Counter regression, inconsistent/corrupt staging, invalid graph or
failed insertion leaves the preceding graph intact. The existing admission lock
fences the check for API-owned operation history: any such history blocks reseeding
that generation. This command must not overwrite API-owned task truth later.

The existing 32 MiB private response bound applies; oversized responses fail
without partial writes or public content truncation. New large-data decoding,
encoding and equality checks run away from Tokio cores. No second executor,
registry, service, authority switch or dependency is introduced.

`adapter-api export-prepared-tasks <generation> <new-path>` writes the verified
prepared graph to a create-new, mode-0600 JSON file. This is a preparation export,
not a complete application backup or an old onchain approval/commitment. It does
not include operation queues, notifications, freezes, money or other owner data.

A prepared snapshot may become stale immediately while provider task writes
continue. Fingerprint/generation equality is not a freshness or cutover proof.
Final activation still requires frontend adoption, paused ingress, reconciled
queued/possibly-submitted work, a fresh verified capture, task workflow/holds and
actual paired task-state rollback. Notification audience timing for other core
Project events also needs the pending explicit owner choice before ingestion
changes or activation. This slice changes neither behavior.

## R3b3a: task executor preparation, with activation withheld

The existing eight-job operation worker can now dispatch the four retained task
commands to a PostgreSQL transaction. A single `task_generations.api_owned` fact
must establish adapter ownership. Its default is false, including upgrades of
prepared generations. Staging rows and `task_operation` history do not activate
execution. No activation command, public admission switch, extra worker pool,
service, queue, dependency or canonical artifact is added. Current public writes
still enqueue provider operations and current public composition stays unchanged.

The executor rechecks operation identity, exact retained request/command bytes,
lease, generation and ownership. It serializes on a project task row, then on the
retained global counter using a non-key lock compatible with foreign-key readers.
The later freeze admission transaction must use that same project row. The four
handlers retain native permission/error ordering, known-worker checks, checked
keys, immutable reporter/creation time and draft/revision deletion rules. A new
empty core storage may be materialized; an obsolete core target is rejected and
missing preparation for a nonempty provider storage fails closed.

Successful task changes, allocator/counts, result, audit and task notifications
commit together. Failed domain execution retains a failed task result without
consuming an ID or sending a notification. Transient failures roll back business
effects and retry the same operation; expired unexecuted work has no task result.
A stale lease cannot commit. Notifications use existing lock 621008 through commit
and the post-change project audience: client, coordinator, contractual assignees
and current task assignees. Every contributing task payload/assignee projection is
validated, including sibling storages. Provider event ingestion remains unchanged;
its separate audience-timing question is unresolved.

Each edited storage retains contiguous task ordinals and vector order. The
original preparation fingerprint remains source provenance, not a hash of mutable
API state. Backfill and `export-prepared-tasks` refuse API-owned generations; the
latter must not present an edited graph as the original prepared capture.

This is deliberately a worker preparation milestone, **not R3 completion or a
supported active cohort**. Tests establish isolated adapter-owned fixtures only.
Do not activate by editing the database. Complete snapshot/edit holds and their
freeze/FIFO ordering, public admission and visibility composition, independent
frontend adoption, actual paired current-task rollback and the audience decision
before exposing an activation procedure. A boolean ownership change cannot
perform that migration or reverse task changes. No live cutover is authorized.
