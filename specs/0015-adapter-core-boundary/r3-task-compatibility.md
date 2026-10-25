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

## R3b3b: immutable snapshots, edit holds and queue recovery

For an adapter-owned generation, accepting a milestone dispute or cancellation
captures the current confirmed API task graph and a Pending hold in the same
transaction as the provider operation and its immutable evidence/reason. The
operation row is locked before the existing project task row, then the non-key
allocator lock. Current core storage associations/revisions and typed task data
are verified before capture. Empty storages and complete retained task values
survive; queued edits are not confirmed data. Identical retries reuse the original
operation/snapshot, conflicting retries fail, and a failed admission rolls back
all of these records together. Snapshot JSON is never changed by completion.

Each operation owns its own hold. A fenced provider save confirms that hold only
with a matching successful receipt. A failed receipt, definitely unsent expiry or
rejection releases it. The existing definitive first `provider_rejected` response
also releases despite its conservative submission marker. Unknown outcomes retain
Pending across expiry and restart; missing receipts or elapsed time do not prove
failure. Operation finality and the hold transition commit atomically. Another
pending or confirmed attempt is unaffected by releasing one hold.

Task execution checks holds before its ordinary expiry: confirmed cancellation
or dispute records the corresponding failed task result; Pending defers the
operation without changing tasks, allocating IDs or manufacturing a receipt.
After release, the original expiry and current permissions apply; waiting does
not grant a fresh lifetime. Confirmed holds take precedence over Pending ones.

Pending-held API tasks are suspended as FIFO blockers across wallets. Remaining
operations preserve their relative order; provider work is never skipped to run
a later freeze. This handles both `task(P) -> other work -> freeze(P)` and the
cross-wallet cycle `A: task(P) -> freeze(Q), B: task(Q) -> freeze(P)`.
Confirmed/released holds stop suspension. An uncertain terminal freeze remains a
wallet blocker rather than being treated as known failure. There is no new
scheduler service or worker pool.

One nullable `operations.task_project_id` is the queue lookup projection. It is
checked against the retained typed command before execution. Startup fills missing
projections only for Queued API-owned work in bounded 64-row Rust decoding pages;
provider-owned future history remains untouched. PostgreSQL JSON extraction was
experimentally rejected because it fails on retained NUL narrative. No SQL JSON
parser, generated column, trigger framework or dependency is introduced.

Planning disputes currently freeze planning funds/status while leaving task edits
available. That behavior is unchanged; the owner question about extending project
holds to planning disputes is still pending. This slice covers cancellation and
all milestone-dispute opening forms, not a silent new planning rule.

Current public task admission/composition and provider-event ingestion remain
unchanged. There is still no activation command. Existing confirmed cases must
be reconciled during the eventual migration without inventing acceptance-time
snapshots. Independent frontend adoption, public task-only visibility and frozen
view composition, notification audience timing and actual paired task rollback
remain activation gates. The holds here are adapter records, not onchain task
approvals or fabricated historical signatures. No live cutover is authorized.

## R3b3c: task membership and frozen-view composition

Provider-owned generations keep the existing task views and R1 copy path. For a
candidate API-owned generation, public readers now substitute only the task
vectors from authoritative PostgreSQL data; core IDs, terms, assignments, status,
capacity, reputation and funds stay unchanged. No generation is activated and
public task admission still uses the provider. There is no activation command.

Ownership, counter, candidate membership, storage contexts and task rows are read
from one short read-only repeatable-read transaction. Current core associations,
IDs, ordinals, retained payloads/assignee arrays and allocator bounds are validated
off the reactor. The staging fingerprint remains provenance, not a checksum of
mutable task state. New empty core storages can render without inserting API rows;
a missing context for nonempty provider tasks fails closed. Another generation's
tasks or stale provider tasks cannot substitute for API-owned truth.

Composition precedes task-based access checks for project/list and task GETs,
task aliases, project briefs, proposal presentation/comments and participant
endpoint access. Detail reads touch only the selected project. Membership arrays
select candidates; typed task payloads must validate before granting access.
Unrelated unauthorized corrupt task content is not decoded or presented. Only
task progress command authorization uses the task overlay, preserving the approved
coordinator-or-assignee rule and client restriction. Other financial/lifecycle
authorization and private dispute reads stay raw. Contractual participant lists,
private channel membership and evaluation voters do not gain task-only members.
Notification title enrichment remains optional and uses current composed
visibility; provider-event ingestion and its unresolved audience timing do not
change. API-owned views bypass R1 task copies rather than writing a new immutable
copy of each task edit. Provider-owned copies are retained until their later
migration/removal.

Public frozen dispute presentation first validates raw core milestone/revision
facts. It then requires exactly one Confirmed opening hold with a matching
Finalized/Success provider receipt, created dispute ID, generation, origin, nonce,
time, retained signed call/command and project/milestone/submission/evidence
context. The captured task graph/context/revision is validated and only the
selected task vector is overlaid. Current API tasks never replace the immutable
accepted snapshot. Pending, released, corrupt or ambiguous evidence fails closed.

For an API-owned generation, a missing accepted snapshot cannot distinguish an
old case from a new direct case with stale provider tasks. The selected public
frozen view therefore fails closed until the explicit migration reconciles that
case. There is no guessed fallback to current, empty or provider task data and no
fabricated historical API acceptance. Provider-owned frozen views and raw private
channel/response authorization remain unchanged.

This is bounded composition preparation, not task activation or R3 completion.
External frontend adoption, active task admission, existing-case reconciliation,
actual paired current-task rollback and the pending planning-dispute/notification
policy choices remain before activation. The subsequent rollback step must restore
actual current tasks and allocator while retaining truthful API operation and
notification history; the prepared-export fingerprint cannot do that conversion.

## R3b3d: paused current-task rollback, native rehearsal

This compatible owner-tool slice restores current API tasks to the preserved
V4 provider cohort. It adds no forward activation, new archive version, public
route, dependency or contract artifact. Provider-owned generations keep their
current behavior. Native proof does not establish Rust/Shadow/Sandbox rollback
or resolve the SDK's unwaived dependency-policy failure.

`adapter-api export-current-tasks <generation> <new-path>` reads the authoritative
current API graph and actual allocator. Current core associations and proposal
revisions select the export; old staging revisions/fingerprints do not describe
mutable truth. Obsolete storage rows remain in PostgreSQL history/backup rather
than being reinserted into current core. Empty current storages are included.
The command requires API ownership, a drained queue across all generations
(including generation-unbound work), no pending freeze, and no unresolved
submission. A definitive `provider_rejected` is known failure despite its
conservative submission marker. An unproven legacy Finalized provider operation
without a receipt blocks this procedure until reconciliation. Files are bounded
32 MiB, create-new and mode 0600, with large data processing off Tokio cores.

`mock-provider restore-tasks <core-archive> <task-state> <new-archive>` validates
both inputs, generation, nonregressing actual counter and every current
project/proposal/revision/storage/milestone association. Missing, extra, duplicate
or invalid contexts fail; no maximum-live-ID inference or truncation is allowed.
Only task vectors and next_task_id change in V4. Whole-state validation follows,
and serializing all other fields must produce identical values: financial and
reputation facts, captured terms, reservations, accepted bytes, nonces, receipts,
events and metadata references are preserved. Unexpected source normalization
fails instead of silently rewriting history. The core archive limit stays 64 MiB.

`mock-provider export <new-archive>` reads its own native SQLite checkpoint through
a read-only connection. It does not open the serving constructor or reseed a
catalog. `import <archive> <new-database-file>` retains its existing fresh-file
requirement. No service reads or writes another service's database.

`adapter-api complete-task-rollback <saved-task-state>` compares the saved export,
current API projection and restored provider's private task-state readback before
setting API ownership false in the same PostgreSQL transaction. A current active
milestone case must also retain the exact public frozen task vector proven by its
confirmed API opening snapshot and real successful signed receipt/context. A
missing, ambiguous or incompatible snapshot fails closed. Unchanged completion is
retryable; failure preserves API ownership and all task/history rows. Only this
ownership fact changes: operations, results, notifications/read state, task data,
allocator/provenance and immutable freeze snapshots remain. No fake provider
receipt, event cursor or historical signature is created.

### Paused owner procedure (no live cutover authorization)

1. Select the exact compatible previous cohort and verify its binary hashes.
   Confirm frontend result/notification compatibility. Pause external ingress,
   all source/core/direct task writers, API workers and provider ingestion;
   reconcile queued/possibly-submitted work. A paused read-only provider endpoint
   may serve owner reads. Take and verify paired PostgreSQL, provider and custody
   backups and preserve the originals. The task file is only a projection, not
   these backups or evidence that the source will remain fresh.
2. Export current API tasks and the provider's current V4 archive, then run the
   provider-owned task converter. Admission/preparation locks are acquired in
   order 621009 then 621007 before queue checks; they fence API admission while
   the tool runs, not direct provider calls or writes between tool invocations.
   Keeping all external writers stopped is an explicit operator precondition.
3. Import into a fresh previous-provider destination. Reopen it, re-export the
   **complete** target archive with the native owner's read-only export command,
   and compare its typed value with the converted archive, including the entire
   non-task history/shell. Do not substitute a task-only comparison for this step.
   Completion's task readback cannot prove preserved money, nonces or history.
4. Point the current owner tool at that verified target and retained PostgreSQL;
   run `complete-task-rollback`. On any mismatch keep ingress stopped and the
   source/backups intact; do not force the ownership flag or truncate data. Start
   the compatible previous adapter/wallet with retained API/custody stores and
   the restored provider. Verify public tasks/cases, result history, mixed SSE
   resume/read state and signed-call replay before reopening ingress.

`python3 scripts/check-task-rollback.py --binaries <current-bin-dir>
--predecessor <frozen-native-bin-dir>` exercises this boundary with disposable
PostgreSQL and SQLite stores, real current services and frozen previous binaries.
The predecessor directory includes its source/hash manifest. The script's SQL
ownership/admission fixture is test-only: public admission still targets the
provider, so the fixture fences a real earlier finalized same-wallet operation,
converts only the just-admitted definitely unsigned task job, then restores the
barrier exactly. The actual SQL worker executes the edits and produces truthful
API task results/notifications. This supplies no production activation procedure.

The scenario changes API tasks without any provider archive effect, retains a
deleted-ID gap, large Unicode/NUL descriptions, assignees, reporters/times and
progress, then opens a real signed milestone dispute and rejects later task work.
It checks fresh import/full archive readback, identical task results and operation
history, frozen public output, notification read state/SSE resume, restart,
existing signed operation replay and further signed task CRUD using the retained
next ID. The disposable PostgreSQL/custody stores are retained through handoff;
this is not a PostgreSQL backup/restore, a live cutover or an all-authority proof.
Existing frontend adoption, active admission, historical-case reconciliation,
policy choices, three-authority task rollback and R4/R5 remain open gates.
