# SPEC-0015 stage 0: ownership and storage inventory

Source: backend `32aadac` plus this read-only inventory example; canonical
runtime `139a01f`. The owner approved the staged plan and both concrete policies
on 2026-10-23: API task snapshot/edit hold, and API-owned catalog names/uniqueness.
There is no authorization to run migration against a live deployment.

## Measurements

Reproduce with the existing workspace dependencies:

```sh
cargo run -p mock-provider --example storage_inventory --locked > /tmp/storage-inventory.json
```

The checked-in `storage-inventory.json` contains exact SCALE sizes from the
existing typed fixtures. Project/Calendar logical root sizes follow the pinned
canonical layout, including fixed-hash ABI reference sizes. They exclude page
wrappers, trie overhead, deposits, allocator cost and gas. UTF-8 sizes are named
explicitly. These are fixtures, not live usage measurements.

| Case / value | Bytes | Interpretation |
|---|---:|---|
| Representative financial fixture, largest Project root | 1,552 | Ordinary fixture stays below 16 KiB already |
| Stress Project root | 280,518 | Includes 200,000-byte Project and 80,000-byte proposal descriptions |
| Stress Project view after clearing text/tasks | 419 | Lower-bound diagnostic; still nested, excludes new metadata commitments |
| Stress worker display name | 261,944 | Presentation data drives the giant worker fixture |
| Stress worker view | 273,488 | Mixes that name with calendar and qualifications |
| Stress catalog entry | 22,017 | Human label exceeds a storage cell by itself |
| Stress calendar root | 9,464 | Legitimate 1,000 overrides and retained reservations still accumulate |
| One retained reservation | 47 | Suitable for an individually keyed record |
| Vector of 1,000 account IDs | 32,002 | IDs alone eventually break aggregate-cell storage |
| Vector of 1,000 public IDs | 16,002 | Already close to the cell cap without useful entity data |

Illustrative encoded header tuples are 159–175 bytes per Project, 98 per worker,
60 per calendar, and 20 per requirement before separately keyed skill membership.
These are feasibility sketches, not the final ABI or a claim that the complete
replacement model is that small. Final core records/inputs are measured again
at R4. Moving descriptions fixes the largest stress values; standard mappings
fix collection growth. Neither requires raising chain storage limits.

## Field and rule inventory

| Current field / dependency | Destination and preserved rule |
|---|---|
| Provider worker display name | Adapter worker-directory content. Preserve its original value separately from mutable principal/profile names; do not assume they are identical. Qualifications, mode, coordinator eligibility and scores remain core. |
| Project title/description, proposal title/description/change-request text, milestone title | Adapter project/proposal content scoped by generation, ID and revision. Agreed content binds to compact commitments. New writes cannot substitute another proposal's text. |
| Proposal presentation/comments, briefs, submission presentation/comments, dispute conversation | Already adapter-owned. Reuse those context stores; do not add a generic document registry. |
| Optional delivery preference | Preserve current draft/revision permissions. Exact-date comparison with milestone durations is a real approval rule: retain a compact date/constraint in core while presentation stays API-owned. |
| Milestone requirement budgets, committed minutes, windows and captured compensation | Core, including zero budgets and integer allocation/overflow rules. Never derive payouts from task estimates/logged minutes. |
| Task content, type/priority/status, assignees, estimates/logged minutes, due date, reporter/timestamps | Adapter tasks. Preserve coordinator-only create/edit, coordinator-or-assignee progress updates, client read-only behavior, known-worker validation and draft/revision-only deletion. |
| Task/storage identity and next task counter | Preserve existing IDs; seed the adapter counter from the retained provider counter, including deleted-ID gaps. New tasks use one transactional adapter allocator. Core no longer needs to retain tracking storage only to render API DTOs. |
| Task membership in `operations::visible` | Adapter visibility must include task-only assignees after R3. Keep contractual assignments independently core-owned. Do not lose access or expose another project's content by clearing provider tasks first. |
| Task assignees in Project event recipients | Adapter notification composition must preserve these recipients. Existing notification IDs are the SSE resume sequence; never invent provider cursors or signing receipts for API-only work. |
| Participant/contact/dispute-channel projections | Preserve their existing audience rules. Project visibility includes task assignees, while the participant list uses approved contractual assignments; do not merge those distinct sets. |
| Submission/dispute state and evidence commitment | Core transition IDs/status, current review targets, author/permission/freeze facts and compact commitments. Narrative/history content stays API-owned. |
| Role/skill labels, category, descriptive associations and moderated request content | Adapter catalog context; preserve current normalization, moderation audience and root-only approvals. Core still validates known role/skill IDs and required worker memberships. Role metadata continues not to filter matching. |
| Fixed coordinator role | Core capability remains role ID 1 and cannot be renamed/deleted through the new core interface. API presents its fixed coordinator label. Do not derive authority from an arbitrary display name. |
| Capacity overrides/reservations | Core mappings per week/reservation, checked totals. Moving calendars to profile JSON would weaken matching and oversubscription enforcement. |
| Reputation/evaluation replay markers | Core exact accumulators and participant/contribution checks. Historical display detail can be projected, without reapplying votes. |
| Balances, escrow, issuance, Bramp status/holds | Core/runtime. Adapter copies do not become authoritative balances or settlement instructions. |
| Root ID vectors and nested entity/history vectors | Individually addressed ordinary ink! mappings and bounded typed reads. Keep only counts/index entries needed for correctness and complete enumeration; no paged encoded root replacement. |

Sources: native `domain/project.rs`, `commands.rs`, `validation.rs`,
`catalog_requests.rs`, `evaluations.rs`; adapter `operations.rs`,
`project_participants.rs`, `notifications.rs`, `disputes/conversations.rs`;
canonical `projects-v5/lib.rs` approval-date validation and `types.rs`.

## Task snapshot and edit-hold policy

The adapter captures confirmed task revisions and creates the pending project
hold in the same PostgreSQL transaction as accepting the dispute request.
Task commits use that same project row lock. Confirmed edits before capture
appear in the snapshot; edits not finalized before capture do not. The snapshot
is immutable even if a rejected dispute later releases its hold.

- Task operations arriving during a pending dispute/cancellation retain the
  public asynchronous operation interface and wait; no chain task transaction.
- Authoritative successful freeze rejects waiting task operations with the
  existing frozen/cancelled-project semantics. Failed or definitely unsubmitted
  expired freeze releases the hold, then operations recheck normal permissions.
- An uncertain provider outcome does not release the hold. Reconcile the actual
  receipt; timeout or restart is not proof that the freeze failed.
- Persist hold, operation identity and snapshot across restart; identical retries
  reuse them, conflicting content is rejected. Reconcile existing confirmed cases
  on migration without fabricating earlier signatures or task snapshots.
- Core money/freeze actions remain independently authorized by contracts.
  Task authority follows confirmed core facts under this API policy; it does not
  claim an atomic order with arbitrary direct contract calls.

This is the expressly approved consistency change. R3 tests must exercise every
race/recovery case above, task-only visibility and notification recipients.

## Catalog policy

Names, request moderation content and duplicate-name validation become adapter
facts. Normalize with the existing Rust implementation: split Unicode whitespace,
join with one ASCII space, then Rust Unicode lowercase. Do not replace it with
PostgreSQL `lower`, locale-dependent collation or a different case-fold algorithm.
Keep the complete normalized text and use the existing SHA-256 implementation
for a fixed-size, generation-scoped unique index key. An unbounded PostgreSQL
text B-tree key would itself fail for the retained long names. Compare full
normalized text on an occupied key; mismatched/colliding content fails explicitly.
No new crypto dependency or PostgreSQL extension is needed. Apply this uniqueness
only to committed skill names, not role labels. Preserve excluding the entry's
own ID for renames. Two pending
requests may share a name, as today; approval/catalog upsert must resolve the
conflict atomically. Do not make pending requests globally unique by accident.

Contracts retain ID existence, privileged creation/approval and qualification
facts, with role ID 1 fixed. A direct chain call cannot establish an API label
without the corresponding validated adapter record. Name validity/uniqueness is
explicitly an API guarantee, not a property inferred from a supplied hash.
Retain full historical labels without shortening them. No tighter public name
limit is needed for R1/R2 just to fit a chain cell after names move offchain.

## Bounds and migration scope

R1 adds copies and composition only. It introduces no new public limits and
changes no provider permission or financial rule. R2 removes text from core;
R4 measures the final fixed/per-entry representation. Qualifications and other
memberships should use keyed entries rather than an arbitrary shortened list.
Retain existing overall admission/gas limits until measured final core inputs
justify a separately approved change. Header sketches do not prove maximum
transaction or full-workflow capacity.

Supported development sources are the current native and candidate ArchiveV1
cohorts. R1 never reads their databases directly and retains the provider source
until each copied record is verified. Actual deployment contents remain unverified;
any later live cutover requires discovery, reconciliation and owner deployment
approval. Paired adapter/core backup and the immediately preceding compatible
release are the rollback boundary, not an all-versions engine framework.
