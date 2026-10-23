# R3a: prepare task consumers before changing ownership

This is a compatible reader milestone within approved SPEC-0015. Task execution,
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
