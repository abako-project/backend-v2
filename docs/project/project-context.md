# Current implementation and limitations

The candidate includes reviewed SPEC-0015 metadata/task ownership and ordinary
ink storage across all three contracts. Astra xhigh accepted the functional R3–R5
implementation, including transitional cleanup. This page describes implementation; [specifications](../../SPECS.md)
define approved behavior. [Dated handoffs](../../progress/handoffs/SPEC-0015.md)
record exact tested revisions and limitations.

## Working implementation

| Area | Current behavior |
|---|---|
| Services | Axum REST adapter, custodial signing wallet and atomic provider; Rust default, embedded WASM candidate explicit |
| Frontends | Independent Leptos CSR and the team's external frontend consume the same REST/JSON and SSE contract |
| Storage | Adapter PostgreSQL, custody SQLite, mock SQLite by default or memory |
| Identity | Password and username-first passkey login, cookie sessions and CSRF; custody signs with sr25519 |
| Marketplace | Worker qualifications and ISO-week capacity, skill matching, coordinator selection, proposals, team assignments and sequential milestones |
| Planning and payments | No separate planning price or transfer. Execution approval reserves the full budget and team capacity upfront. Each accepted milestone pays its stored shares. |
| Compensation | Configurable coordinator percentage, initially 10%; remaining budget allocated by planned worker minutes with exact integer totals |
| Tracking | One task storage per proposal milestone, preserved through draft edits. Storages may remain empty through submission and execution. Tasks do not determine payouts. |
| Descriptive data | Immutable directory names, Project root and proposal text, milestone titles, change explanations, catalog/request narrative and lifecycle reasons, client/worker profiles and images, briefs, proposal presentation, review comments and submission/rejection supporting content in adapter PostgreSQL |
| Evaluations | Final 1–5 ratings under approved party rules, separate from payments; minute-weighted reputation |
| Disputes | Current rejection permits public opening and freezes the project. Append-only public arguments and a private case conversation are implemented. |
| Events | Durable adapter operations, receipt reconciliation, event ingestion and recipient notifications over authenticated SSE |
| Platform | Public deployment-specific branding, currency display metadata, project types and terminology; software/Abako default, integer mock currency with decimals 0 |
| Development | Local Compose/Nginx at localhost:8088; real-service API E2E with disposable PostgreSQL in both mock modes |
| Delivery | PR/master application checks, tag-triggered backend images and an approved mock-backed HTTPS preview; see [cloud status](../operations/cloud.md) |

Matching requires every requested skill, active worker/coordinator mode and
capacity. Roles do not filter matching. Eligible previous-milestone workers are
preferred before reputation ranking and random exact ties. Execution approval
assigns every milestone atomically; only the first activates, and acceptance
activates the next until the project is completed.

The mock owns compensation policy. Draft creation/update captures current shares;
submitted and approved amounts remain fixed. Coordination minutes count for
capacity and reputation. Final reviews do not move money. Detailed behavior and
pending evaluation-weighting decisions are in the [happy path](happy-path.md)
and [SPEC-0003](../../specs/0003-transactional-marketplace/spec.md).

## Mock and Kreivo

The default mock validates signed custom SCALE calls and executes Rust business
rules atomically. SQLite persists core facts, nonces, receipts and events;
memory mode is disposable. It produces no parachain blocks.

The explicit [ink! v5 candidate](../../tools/ink-sandbox/README.md) executes
source-pinned Directory, Calendar and Project WASM inside one native runtime.
Rust, Shadow and Sandbox authority share the existing private HTTP boundary;
the native reference remains available for differential checks. This candidate
includes full application-domain parity, durable restart and current-state
import/rollback. Its SDK dependency policy currently fails on unwaived RUSTSEC-2026-0387
(`crypto-bigint`); earlier scoped maintenance/license exceptions do not cover it. It is not the deployed Kreivo contract cohort or a real-chain
provider; changing an endpoint URL cannot supply compatible execution and
reconciliation. ink! v6 remains outside scope.

SPEC-0015 makes the adapter authoritative for descriptive content, catalog names
and uniqueness, task text/progress and accepted dispute snapshots. Core V5 keeps
fixed references, numeric terms, permissions, task IDs/assignees and the actual
global allocator. Creation/deletion and membership changes use real signed core
operations; local content edits return truthful API task results without invented
provider receipts. Project notification audiences reflect membership at core
execution. Uncertain operations retain their intents and block conflicting work;
pending dispute snapshots hold later task edits while the outcome is uncertain.
Confirmed disputes retain the core lifecycle edit guard.

R3 includes guarded activation and paused V4↔V5 paired owner migration/rollback,
preserving original content, frozen cases, IDs and history. Missing content fails
closed. Current views have no predecessor text fallback. See the
[owner procedure](../operations/task-cutover.md); existing data cannot silently
switch ownership, and frontend compatibility must precede live activation.

R4 replaces custom paging with ordinary ink storage in Calendar, Directory and
Project. Fixed records and individually addressed growing lists stay below the
unchanged cell limit. Fresh typed imports are bounded, verified and permanently
sealed. Complete current archives and signed replay are tested against the frozen
preceding executable. Whole-history scans still have finite gas/memory costs.

R5 removes the unused storage helper, three obsolete proof contracts and the
Calendar-only signed bridge. The bundle retains three application contracts and
one private failure probe. Unique regression checks move to current contracts;
Git provenance, frozen binaries, native financial reference and required offline
recovery codecs remain. The [cleanup inventory](../../specs/0015-adapter-core-boundary/r5-cleanup.md)
maps removals and surviving checks. No database records are deleted.

Controlled-host performance acceptance, dependency-policy remediation, independent
frontend deployment, authority promotion and observed releases remain open.
No image publication, deployment or onchain upgrade is authorized by this work.

## Remaining limits

- No real-chain transactions, real assets, bank integration, currency conversion,
  DAO governance/voting or generic external payments. Bramp models deposits and
  withdrawal holds with operator confirmation.
- Formal dispute rulings, resolution payments, refunds and unfreezing are not
  implemented. Conversation access defaults to client/coordinator; the existing
  `DISPUTE_CHANNEL_ALLOW_PARTICIPANTS` option includes assigned workers. Public
  arguments remain client/coordinator-only.
- Proposal dates retain a global preference/date and separate ISO-week windows.
  Per-milestone specific dates and time-extension requests remain pending.
  The target date is advisory; it does not trigger penalties or payments.
- Platform configuration describes one profile per deployment. It does not
  establish tenant isolation, DAO membership or automatic asset metadata queries.
- Custody encrypts seeds using a file-backed master key. HSM/envelope encryption,
  online key rotation and production recovery policy remain future work.
- The fresh dependency check reports six vulnerability advisories in locked TLS
  dependencies, plus license-policy and maintenance findings. These and production
  supply-chain gaps remain explicit in
  [supply-chain status](../security/supply-chain.md). Package metadata and the
  template license notice need an owner decision before being treated as a settled
  project licensing statement.

## Evidence and next work

The [simplification execution record](repository-simplification.md#execution-record--2026-10-21)
records current local verification, draft review links and the pending Docker
gateway/browser acceptance. No runtime or public contract changed during cleanup.
The [platform handoff](../../progress/handoffs/PLATFORM-001.md) and
[porting review](porting-coverage.md) retain historical evidence for their original
revisions. The [sandbox consolidation](../../progress/handoffs/SPEC-0013-CONSOLIDATION.md)
records the embedded candidate; the [ownership handoff](../../progress/handoffs/SPEC-0015.md)
records current ownership source, exact checks, Astra review and later milestones.

### Independent frontend readiness

The independent `abako-web/dev` frontend compatibility patch is prepared in
[draft PR #55](https://github.com/abako-project/abako-web/pull/55), revision
`0fa37b0adebecd57f7f8b95db5d78764d4c13f54`. Astra xhigh accepted its task-result
binding/recovery and mixed notification/SSE readers; all 429 tests pass. This is
reviewed source readiness, not deployment or browser-to-new-writer evidence.
