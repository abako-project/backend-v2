# SPEC-0015: Simplify adapter and contract ownership

Status: APPROVED.
Approval: owner approved the staged plan in the current conversation, 2026-10-23.
Task: SPEC-0015. Owner requested a simplification plan on 2026-10-23 after
identifying custom storage paging as a symptom of inappropriate provider data.

## Decision

The adapter owns application content and workflow that does not determine
funds, capacity, reputation or contractual lifecycle. Contracts retain the
smallest state needed to enforce those rules independently. Public REST/JSON/SSE
views are assembled by the adapter; they are not contract storage schemas.

Use ordinary ink! storage fields and `Mapping` entries for bounded core records.
Remove `storage-v5` and its root-blob paging once consumers have migrated.
Keep ink! v5, one embedded runtime and the existing adapter/wallet/provider
processes. Add no service, document framework, generic repository, storage engine
or cross-database transaction coordinator.

This proposal changes internal ownership and compatibility requirements from
SPEC-0003/0006/0013. Their approved business rules remain authoritative until
explicit amendments are approved. SPEC-0014's evidence commitments remain valid.
Plan approval does not settle the stage-0 policy decisions below or approve
unrelated permission changes, tighter public limits, historical
record deletion, release publication or deployment.

## What caused the current complexity

The native mock's combined read/storage models were copied into contracts to
preserve every accepted state. Project contains nested proposal, milestone,
task, submission and reason histories. Calendar contains a growing reservation
vector. Directory and Bank keep growing identifier vectors.

Compatibility fixtures include a near-256 KiB worker display name, 22,000-byte
catalog names, a 200,000-byte project description and an 80,000-byte proposal
description. Those are deliberate stress fixtures, not measured production
requirements. They demonstrate overly broad native admission; they do not
justify storing presentation content onchain.

Two problems need different fixes: move descriptive content to the adapter,
and store legitimate growing collections as individually addressed records.
Even compact records eventually exceed a cell limit when packed into one vector.
Removing strings alone will not remove that aggregate growth.

Evidence: `services/mock-provider/src/domain/financial_parity.rs`,
`tools/ink-sandbox/tests/capacity.rs`, canonical `projects-v5/types.rs`,
`projects-v5/lib.rs`, `directory-v5/lib.rs` and `calendar-v5/lib.rs`.

## Proposed ownership

| Data or rule | Owner after migration | Contract requirement |
|---|---|---|
| Display names, profiles, project/proposal/milestone titles and descriptions | Adapter PostgreSQL | IDs and versioned content commitments only where needed to bind agreed terms |
| Briefs, presentations, review comments, written reasons, delivery text and evidence preimages | Adapter, reusing current context tables | Existing lifecycle/evidence IDs and commitments; no duplicate narrative history |
| Tracking tasks: content, status, assignees, estimates, logged minutes and task CRUD | Adapter, subject to the task-policy decision below | No task engine by default; project and milestone control facts remain authoritative |
| Proposal commercial terms | Contracts | Revision/status, integer budgets, committed minutes, requirements, capacity windows, captured compensation and assignments |
| Delivery constraints used by approval | Contracts | Compact typed values sufficient for current date/schedule checks; they cannot be moved merely because they look descriptive |
| Execution, cancellation, submissions and disputes | Contracts | Authorization, current transition targets, freeze/settlement state and compact commitments; presentation/history in adapter |
| Qualifications, worker mode, coordinator eligibility and reputation | Contracts | IDs/membership, exact accumulators and replay markers used by selection and voting |
| Catalog labels and requests | Split after name-rule design | IDs, qualification relationships, fixed coordinator capability and normalized-name uniqueness must remain enforceable |
| Calendar capacity and reservations | Contracts | Per-week/per-reservation records, checked totals and reservation identity |
| Balances, escrow, issuance, deposits and withdrawals | Contracts/runtime | Exact amounts, authorization, status and replay protection |
| Search/listing/history projections | Adapter | Retain core facts/markers needed for correctness and complete enumeration; avoid duplicated full view snapshots |
| Signing and execution envelope | Existing wallet/provider shell | Keys, signatures, generation, nonce, idempotency, receipts and durable execution remain in their current owners |

Tasks do not determine payouts or reputation. Those use approved requirement and
coordination minutes. Task assignees differ from contractual worker assignments.
The existing adapter already owns briefs, proposal presentations, submission
presentations, comments and conversations; extend those contexts rather than
introducing a universal content store.

## Compatibility and data rules

- Keep frontend routes, JSON fields, IDs, asynchronous operation polling, SSE
  payloads/resume behavior and visibility. Rebuild public project/worker/task
  views by joining core facts with adapter-owned records.
- Separate application field validation from contract rule enforcement. Contracts
  must still reject unauthorized money/lifecycle changes without trusting an
  adapter snapshot or an opaque document hash.
- A proposal commitment binds the immutable human terms and the corresponding
  core terms/revision. Contracts still receive and validate the actual numeric
  and scheduling terms needed for approval and settlement. A hash is not a
  substitute for those checks.
- Scope metadata by provider generation and entity/revision. Confirmed content
  must resolve to the signed commitment. Pending writes are not confirmed
  public state. Reuse the existing persistence/queue pattern from SPEC-0014.
- Missing content is explicit. Preserve access to core money/lifecycle operations
  that do not require it; never invent descriptions, substitute another revision
  or let metadata failure become a different financial result.
- Read only through owner-provided exports/interfaces for backfill. Neither
  provider nor wallet reads adapter PostgreSQL; the adapter does not read their
  database tables directly.
- Retain original IDs, authors, timestamps, historical text and accepted amounts.
  Do not truncate old data, replay payments or fabricate historical commitments.
  A migration-time commitment must be identified as such, not as an old approval.
- Use additive adapter migrations. During backfill, provider data stays the
  source for records not yet verified. After cutover, adapter data is authoritative;
  keep fallback temporary and explicit, never a permanent pair of writers.
- Backups and rollback must include both the current core state and adapter-owned
  content. A provider checkpoint alone will no longer represent the full app.
- Support a tested transition from the current cohort to the next compatible
  release. Do not preserve arbitrary oversized mock inputs, every old storage
  layout or every historical engine indefinitely. Reconcile queued/signed work
  before changing internal codecs; never silently reinterpret signatures.
- Tighter public input limits need explicit approval and contract documentation.
  Existing long descriptions can remain intact in PostgreSQL. Core bounds must
  come from real workflows and size measurements, not invented truncation rules.

## Staged delivery

Estimates are working days for implementation and review, not release promises.
Allow roughly four to six weeks overall; measure and adjust after stage 0.
Every runtime release must pass the existing frontend flows before handoff.
Astra xhigh reviews each exact implementation milestone before acceptance.
Publishing/deploying each release remains a separate owner action.

| Stage | Scope and estimate | Working result / exit check |
|---|---|---|
| 0: agree boundaries | 1–2 days | Field/rule inventory, actual SCALE sizes for representative and retained fixtures, proposed bounds and resolved decisions below. Approved amendments before dependent implementation. No runtime change. |
| R1: adapter copies and public composition | 3–4 days | Add explicit context tables/columns only where needed; idempotently backfill current provider data and compare assembled views. Provider remains authoritative during this stage. Existing frontend, old data and rollback still work. |
| R2: metadata ownership | 4–6 days | Adapter becomes the writer for descriptive fields. Extend compact commands and versioned commitments for agreed documents, keeping numeric/lifecycle checks in both native and WASM core. Route reads through adapter composition. Large text round trips without entering contract state; task workflow is unchanged. |
| R3: adapter task workflow | 4–6 days | Move task CRUD/validation and nonfinancial workflow into adapter under the approved task policy. Preserve task/storage IDs, authors/timestamps, operation responses, notifications, aliases and frozen case context. Demonstrate edits cannot alter money, reservations or reputation. No chain transaction per tracking edit is the recommended target. |
| R4: ordinary ink! core storage | 5–8 days | Replace root blobs and growing vectors with standard fields and keyed mappings. Use bounded typed queries/import records. Update native core/SDK codecs to the smaller model, rebuild the three v5 artifacts, migrate into fresh sealed contracts and prove current-state rollback to the immediately preceding compatible release. No paging helper remains in current execution. |
| R5: remove transitional code | 2–3 days | Verify backfill completeness, then remove obsolete provider metadata/task storage and adapter fallback, custom paging/text codecs, duplicate full-view ABI conversion and unused compiled fixture wiring. Keep unique regressions as tests. Document final ownership and backup/restore. |

R1 can be released without changing authority. R2/R3 need migration and compatible
native/WASM command handling before serving the changed write paths. Each stage
must work with the authority modes it advertises; an incomplete candidate remains
private, not silently bypassed. R4 is the deliberate storage-cohort transition;
retain R3 as its compatible rollback target. Avoid an all-versions conversion
framework. R5 does not retire the native financial reference or waive the existing
promotion/two-observed-release gates.

Split R2 by profile/project content versus proposal documents if actual review
scope exceeds one week. Split R4 by contract if that gives independently complete,
working cohorts; never publish a mixed layout that cannot export or roll back.

## Stage 0 decisions required before dependent implementation

1. **Task authority and freeze ordering.** Recommended: task workflow becomes
   adapter-owned, preserving the coordinator/assignee/client permission matrix
   and immutable case context. Today its exact ordering with a dispute freeze
   comes from one provider transaction. Two databases do not preserve that
   property automatically. Specify task edits racing cancellation/dispute,
   pending operations, snapshot capture and recovery before R3. Explicitly approve
   any changed ordering/trust guarantee. Do not invent a distributed lock service
   or an onchain task guard merely to claim the old atomicity was preserved.
2. **Catalog naming.** Current rules normalize Unicode skill names, reject
   duplicates and fix coordinator role ID 1. Keep those rules until an approved
   compact representation preserves them. Moving labels or accepting a supplied
   hash alone does not prove normalization/uniqueness. Resolve new-name bounds
   and legacy long labels before removing their paging path.
3. **Real core bounds.** Measure per-record and per-command encoding after text
   removal. Define any business limits needed for milestones, requirements,
   qualifications and dates. Prefer keyed membership/reservation records over
   arrays that grow with all users/history. No limit increases or silent drops.
4. **Deployment data and rollback scope.** Identify active environments, pending
   operations and retained data to preserve. Use one concrete supported migration
   and compatible rollback path. New adapter-only operations must not fabricate
   provider nonces/receipts/events or collide with existing public SSE cursors.

These are design gates, not permission to guess missing rules. Independent R1
planning can proceed while they are resolved; affected implementation cannot.

## Acceptance

Record before/after field ownership, encoded record/input sizes and the custom
code removed. Total state may grow, but an individual core record must stay
bounded below the existing cell limit with documented headroom. Test many ordinary
records, not only giant text fields. No runtime limit increase, hidden payload
fragmentation or replacement storage framework is an acceptable outcome.

For every release, verify public REST/SSE and existing UI workflows, IDs and
visibility; permissions/freeze and revision races; idempotency and lost responses;
missing/tampered content; financial conservation and exact reputation; migration,
restart, failed-commit recovery and current-state rollback. For task cutover also
verify global task-ID allocation, both route forms, assignee updates, draft-only
deletes, visibility, notifications and immutable dispute snapshots.

Move presentation parity assertions to adapter/API tests. Contract differential
tests compare enforceable core facts and commitments. Keep exact money,
authorization and failure regressions. Oversized descriptive fixtures become
adapter persistence/round-trip tests instead of requirements for giant WASM
storage values; do not retain onchain fields solely to match old full-view dumps.

Use applicable `ci/README.md` app/SDK gates and the unchanged dependency policy.
Reproduce changed v5 WASM from committed source. At R4, inspect final dependencies
and artifacts to verify current contracts use standard ink! storage and no
`storage-v5`; compare warm engine latency and storage work on a controlled host.
Do not represent prior paging-candidate timing as evidence for this design.

The final shape is the existing adapter for app content/workflow, wallet for
signing and a small contract-backed core for enforceable business facts, with
three operational contract types and no custom storage system.

## Stage-0 policy decisions approved 2026-10-23

Owner selected API snapshot and edit hold. Task policy: the adapter captures an immutable snapshot of confirmed task data
when accepting a dispute request. Subsequent task operations wait while that
freeze/cancellation is pending; successful freeze rejects them under the existing
frozen-project rule, failed freeze releases the hold. Ordinary task edits need
no onchain transaction. Authorization uses confirmed core facts, preserving the
existing actor matrix. This defines an API consistency boundary, not atomicity
with arbitrary concurrent direct contract calls. The owner explicitly approved the changed ordering relative to the
old single-provider transaction.

Owner selected adapter-owned names and uniqueness. Catalog policy: adapter PostgreSQL owns names, requests and name uniqueness,
using exactly the current Rust whitespace-collapse/Unicode lowercase semantics
and a transactional unique key (fixed-size digest plus full normalized-text
comparison, retaining long names without oversized B-tree keys). Contracts own catalog IDs, qualification facts
and admin authority; coordinator role ID 1 remains fixed. Label validity and
uniqueness become adapter guarantees, not independently checked chain properties.
No arbitrary shortened-name rule is introduced. The owner explicitly approved this boundary.
