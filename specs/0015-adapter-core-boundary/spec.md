# SPEC-0015: Simplify adapter and contract ownership

Status: APPROVED.
Approval: owner approved the staged plan in the current conversation, 2026-10-23.
Task: SPEC-0015. Owner requested a simplification plan on 2026-10-23 after
identifying custom storage paging as a symptom of inappropriate provider data.

## Fresh-install simplification amendment

Approved by the owner on 2026-10-26: the only deployed environment is disposable
development data. Start from clean state and minimal current fixtures; historical
migration, rollback, legacy formats and intermediate deployment compatibility are
no longer requirements. This amendment supersedes conflicting historical-data,
additive-migration, bridge-release and maintained-reference obligations in
SPEC-0013/0014/0015. Keep current frontend REST/JSON/SSE behavior, authorization,
exact money, contract-enforced task identity/membership and API-owned content.

Use one current internal model and schema. Remove historical import/export
converters, staged ownership switches, migration-only commitments and fixture
copies. Initialize fresh state through ordinary constructors/current commands.
Retain restart persistence, operation idempotency, uncertain-outcome recovery,
atomic commits and ordinary backups of a running current installation. Incompatible
old stores must fail clearly; startup must never silently reset an existing store.

The contract provider should execute one WASM authority and read its own current
facts/shell. Remove live shadow/native comparison and its duplicated state. A
standalone native development provider may remain isolated while the existing SDK
dependency-policy gate prevents normal application integration; it is not a legacy
migration or runtime-read dependency. Remove unnecessary duplicate representations
without introducing another service, generic repository or storage framework.

Keep focused current financial, permission, task/freeze, signature, restart and
failure-atomicity tests. Replace historical golden archives with small fresh
scenarios. Record source changes, measured reduction and review in the existing PRs.
No dependency-policy waiver, image publication or deployment is authorized.

## Calendar consolidation amendment (2026-10-27)

The owner approved folding Calendar into Dao as an internal module. The two
business contracts are Dao and Project; the private VM probe remains test-only.
Use ordinary account-keyed ink mappings for capacity, overrides, reservation
history and totals. Remove separate Calendar deployment, code pins and addresses.
Preserve public calendar IDs/views, owner-only capacity edits, authorized Project
reservations, matching order and atomic all-or-nothing team approval. Calendars
remain scoped to one DAO; cross-DAO shared capacity is not introduced.

This supersedes the three-contract topology below, not its business rules.
Continue using fresh development state and reject incompatible checkpoints.
Measure the combined code size and reservation execution work under unchanged
runtime limits; retain restart, competing reservations and failure regressions.

## Decision

The adapter owns application content and workflow that does not determine
funds, capacity, reputation or contractual lifecycle. Contracts retain the
smallest state needed to enforce those rules independently. Public REST/JSON/SSE
views are assembled by the adapter; they are not contract storage schemas.

Use ordinary ink! storage fields and `Mapping` entries for bounded core records.
Remove the custom root-blob paging helper once consumers have migrated.
Keep ink! v5, one embedded runtime and the existing adapter/wallet/provider
processes. Add no service, document framework, generic repository, storage engine
or cross-database transaction coordinator.

This proposal changes internal ownership and compatibility requirements from
SPEC-0003/0006/0013. Their approved business rules remain authoritative until
explicit amendments are approved. SPEC-0014's evidence commitments remain valid.
The approved policy decisions are recorded below. Approval does not authorize
unrelated permission changes, tighter public limits, historical
record deletion, release publication or deployment.

## Ownership

| Data or rule | Owner after migration | Contract requirement |
|---|---|---|
| Display names, profiles, project/proposal/milestone titles and descriptions | Adapter PostgreSQL | IDs and versioned content commitments only where needed to bind agreed terms |
| Briefs, presentations, review comments, written reasons, delivery text and evidence preimages | Adapter, reusing current context tables | Existing lifecycle/evidence IDs and commitments; no duplicate narrative history |
| Task text, status, estimates, logged minutes and other descriptive fields | Adapter PostgreSQL | Financial and lifecycle checks remain independent of task content |
| Task IDs, allocation, storage context and assignees | Contracts, under the 2026-10-26 amendment | Creation/deletion and assignment changes execute on-chain; preserve the existing permission matrix and notification membership at core execution |
| Proposal commercial terms | Contracts | Revision/status, integer budgets, committed minutes, requirements, capacity windows, captured compensation and assignments |
| Delivery constraints used by approval | Contracts | Compact typed values sufficient for current date/schedule checks; they cannot be moved merely because they look descriptive |
| Execution, cancellation, submissions and disputes | Contracts | Authorization, current transition targets, freeze/settlement state and compact commitments; presentation/history in adapter |
| Qualifications, worker mode, coordinator eligibility and reputation | Contracts | IDs/membership, exact accumulators and replay markers used by selection and voting |
| Catalog labels and requests | Adapter owns names, requests and normalized-name uniqueness under the approved policy below | Core owns IDs, qualification facts, skill-to-role references needed to prevent deleting a referenced role, fixed coordinator capability and admin authorization |
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
Reproduce changed v5 WASM from committed source. Inspect final dependencies
and artifacts to verify current contracts use standard ink! storage with no
custom paging helper; compare warm engine latency and storage work on a controlled host.
Do not represent prior paging-candidate timing as evidence for this design.

The final shape is the existing adapter for app content/workflow, wallet for
signing and a small contract-backed core for enforceable business facts, with
three operational contract types and no custom storage system.

## Policy decisions approved 2026-10-23

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

## Execution-time notification audience amendment (2026-10-26)

The owner chose to preserve task-assignee membership at core execution time for
Project notifications, then explicitly approved confirming membership changes
on-chain. Task text and progress remain adapter-owned; adding/removing assignees
requires a core transaction. This supersedes the earlier expectation that every
tracking edit is API-only. Reconstructing recipients at ingestion time or from
cross-store timestamps does not meet this choice.

The implementation must use the existing operation/signing/reconciliation path,
not another coordinator or queue. A membership change cannot be reported complete
before core confirmation and durable API materialization. Uncertain outcomes
must preserve the exact intent and block conflicting work. Classification of an
edit must use the serialized execution state, not a potentially stale admission
snapshot. An accepted dispute snapshot cannot race an unresolved membership
change: reconcile it before accepting the dispute, without holding a database
transaction across signing or core submission.

The owner subsequently approved retaining task IDs and assignees in core,
including on-chain creation/deletion for unassigned tasks. Core retains the actual
global allocator, contextual task identity and membership; the adapter owns
narrative, progress and other descriptive fields. This supersedes the provisional
API allocator/CRUD ownership, avoiding a second reservation/recovery protocol.
Retain existing IDs and error/permission rules. Keep provider receipts for core
operations and truthful API task results for local edits. A task creation's
private execution record must identify its allocated task ID unambiguously for
recovery; do not infer it from a later counter or change the public receipt's
existing storage-ID field.

No generation is activated by this amendment and frontend adoption remains a
cutover prerequisite. Existing API-only execution fixtures do not establish
compliance with this new membership requirement.

## Current task visibility and result rules

These consolidate the approved task decisions formerly recorded in the staged
reader notes; they do not change behavior. Planning disputes preserve task editing.
A pending milestone dispute/cancellation holds subsequent task edits while its
outcome is uncertain. A confirmed freeze rejects later edits; a failed operation
releases its hold. Unresolved membership changes block snapshot admission.

Task-assignee visibility and Project event audiences are distinct from contractual
participants, private-channel access and evaluation eligibility. Membership is
confirmed in core; notification recipients reflect core execution. API task changes,
results and notifications commit together. They use no invented provider receipt
or cursor. Completed operations have exactly one result source, receipt or
`taskResult`; missing/conflicting results remain unresolved. Both notification
sources share the outer notification ID for SSE resume and read state.

Metadata ownership and context binding are documented in [creation metadata](creation-metadata.md),
[proposal metadata](proposal-metadata.md) and [catalog/lifecycle metadata](catalog-metadata.md).
The data-only codecs and committed vectors define the exact signed byte encodings.
