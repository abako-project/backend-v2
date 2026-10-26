# Task ownership handoff

This is the explicit V4→V5 task migration described by
[SPEC-0015](../../specs/0015-adapter-core-boundary/spec.md#execution-time-notification-audience-amendment-2026-10-26).
V5 here identifies the private archive/engine format; contracts still use ink! v5.
Current verification is recorded in the [review PR](https://github.com/abako-project/backend-v2/pull/24).
This procedure does not authorize production activation or deployment.

## Preparation

Verify frontend support for `taskResult`, `Queued` and `AdapterTask` notifications.
Keep the exact preceding adapter/provider executables and their hashes. Preserve
coherent backups of each owner's store and all API metadata/history. A provider
archive or task JSON file alone is not an application backup.

Pause public admission, adapter workers, and every other path that can submit core
writes. Reconcile submitted/uncertain operations using the currently serving
cohort before stopping its workers. A queue check cannot fence direct core calls;
the operator must enforce that pause. Unresolved operations or pending holds block
the handoff. Keep the same provider generation and configured root throughout.
Use protected new output paths on disk, not RAM-backed build/scratch directories.

## Activate task content

Run the new adapter owner commands against its existing PostgreSQL store and the
paused V4 provider's private HTTP endpoint:

```sh
adapter-api prepare-task-cutover
adapter-api export-prepared-tasks GENERATION NEW_TASK_STATE_JSON
```

Preparation verifies full task content, actual allocator and historical frozen
case context without transferring ownership. Export the paused V4 provider using
its preceding executable. Convert with the new mock-provider owner command:

```sh
mock-provider compact-tasks V4_ARCHIVE NEW_TASK_STATE_JSON NEW_V5_ARCHIVE
```

This requires exact agreement with the prepared content. Import the V5 archive
into a **fresh** destination using the chosen provider's owner interface. For the
native provider use `mock-provider import NEW_V5_ARCHIVE NEW_DATABASE_FILE`; the
SDK uses `whole-provider import NEW_V5_ARCHIVE` with a new configured database path.
Do not point a new serving binary directly at a V4 database. Reopen the fresh
V5 destination, export its complete archive to another new file using that
provider's owner interface, and structurally compare the full decoded archive
with `NEW_V5_ARCHIVE`. Compare every field with exact integers, including funds,
reservations, receipts/events and history; task views alone are insufficient.
Do not proceed unless import, reopen and the complete comparison all succeed.

Start only the V5 private provider, point the stopped adapter's provider
configuration at it, then run:

```sh
adapter-api activate-task-content
```

Activation checks generation, actual counter, selected storage graph, prepared
content and complete frozen-case inventory before changing API ownership. Resume
the adapter and ingress only after the owner approves the verified cohort.
Do not activate by changing a database flag. Fresh empty development cohorts use
a separate startup check that rejects any retained task or active-case history.

## Roll back current state

Pause and reconcile all writers again. With the adapter configured against the
current V5 provider, export current task content:

```sh
adapter-api export-current-tasks GENERATION NEW_CURRENT_TASK_STATE_JSON
```

Export the current V5 core archive through that provider's own owner interface.
Restore the paired task data with the new converter:

```sh
mock-provider restore-tasks V5_ARCHIVE NEW_CURRENT_TASK_STATE_JSON NEW_V4_ARCHIVE
```

Import the result into a fresh destination with the frozen V4 provider executable.
Reopen that destination with the same frozen executable and export its complete
archive to another new file. Structurally compare the complete decoded V4 archive
with `NEW_V4_ARCHIVE`, retaining exact integer values and every field. This must
include balances, escrow/holds, reputation, reservations, signed receipts, events
and all other core history. The later task-only HTTP checks do not replace this
gate. If the frozen target cannot import/reopen exactly or exceeds its supported
capacity, keep writes paused and ownership unchanged. Never truncate state or
switch to an unverified target to get past an import failure.

Only after that complete comparison passes, point the stopped adapter at that restored private endpoint and run:

```sh
adapter-api complete-task-rollback NEW_CURRENT_TASK_STATE_JSON
```

Completion requires exact current task content, actual IDs/allocator/membership,
storage context and full frozen case views. Missing, changed or extra active cases
fail closed. Failure leaves API ownership unchanged; keep writes paused and repair
the input/cohort mismatch before retrying. Never resume an old provider merely
because its import succeeded.

Keep PostgreSQL operation, task-result, notification, snapshot and provenance
history. After successful completion, use the preserved compatible adapter and
V4 provider pair. Verify public views, operation replay, notification read/SSE
state, restart and further task CRUD before resuming ingress. No destructive
cleanup of prior stores is part of this procedure.

Preparation/activation retries are idempotent for this handoff. This procedure
supports one forward migration and current-state rollback. After rollback resumes
V4 writes, `prepare-task-cutover` deliberately refuses to reseed a generation with
API task history; it is not a repeated roll-forward procedure. Preserve those
histories and resolve a subsequent upgrade explicitly rather than clearing them.

## Local rehearsal

`scripts/check-task-rollback.py` exercises the paired current-state handoff using
frozen predecessor binaries. It supports the native mock and SDK Rust, shadow and
sandbox modes. This includes current writes, frozen case views, retained history,
restart, replay and further V4 task CRUD; see its `--help` for executable paths.
