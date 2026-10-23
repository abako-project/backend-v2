# Current implementation and limitations

Application baseline: `608c70b`; current candidate includes SPEC-0015 R2b1,
2026-10-23. This page describes implementation;
[specifications](../../SPECS.md) define approved behavior. Dated handoffs record
verification of their own revisions, not a fresh verification of every later commit.

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
| Descriptive data | Immutable directory names, Project root and proposal text, milestone titles, change explanations, client/worker profiles and images, briefs, proposal presentation, review comments and submission/rejection supporting content in adapter PostgreSQL |
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
import/rollback. Its reviewed SDK graph passes the separately approved scoped
dependency policy. It is not the deployed Kreivo contract cohort or a real-chain
provider; changing an endpoint URL cannot supply compatible execution and
reconciliation. ink! v6 remains outside scope.

SPEC-0015 R2a/R2b1 moves immutable directory names, Project root and proposal
text, milestone titles and change explanations into the adapter. Both cores keep
fixed references and actual numeric proposal terms; public views stay unchanged.
Existing V2 stores require the explicit [paired V2↔V3 migration and rollback procedure](../../specs/0015-adapter-core-boundary/r2b-proposals.md).
Catalog names/moderation and cancellation/planning-dispute reasons remain R2b2;
tasks remain R3 and removal of the paging helper R4/R5. Formal performance acceptance, promotion and two observed
stable releases remain open. No image publication, deployment or onchain upgrade
is authorized by this candidate.

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
records the current R2b1 source, exact checks, Astra review and later milestones.
