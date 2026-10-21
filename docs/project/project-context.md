# Current implementation and limitations

Reviewed against `608c70b` on 2026-10-21. This page describes implementation;
[specifications](../../SPECS.md) define approved behavior. Dated handoffs record
verification of their own revisions, not a fresh verification of every later commit.

## Working implementation

| Area | Current behavior |
|---|---|
| Services | Axum REST adapter, custodial signing wallet and atomic Rust mock provider |
| Frontends | Independent Leptos CSR and the team's external frontend consume the same REST/JSON and SSE contract |
| Storage | Adapter PostgreSQL, custody SQLite, mock SQLite by default or memory |
| Identity | Password and username-first passkey login, cookie sessions and CSRF; custody signs with sr25519 |
| Marketplace | Worker qualifications and ISO-week capacity, skill matching, coordinator selection, proposals, team assignments and sequential milestones |
| Planning and payments | No separate planning price or transfer. Execution approval reserves the full budget and team capacity upfront. Each accepted milestone pays its stored shares. |
| Compensation | Configurable coordinator percentage, initially 10%; remaining budget allocated by planned worker minutes with exact integer totals |
| Tracking | One task storage per proposal milestone, preserved through draft edits. Storages may remain empty through submission and execution. Tasks do not determine payouts. |
| Descriptive data | Client/worker profiles and images, project briefs, proposal presentation, scope-review comments and submission/rejection supporting content in adapter PostgreSQL |
| Evaluations | Final 1–5 ratings under approved party rules, separate from payments; minute-weighted reputation |
| Disputes | Current rejection permits public opening and freezes the project. Append-only public arguments and a private case conversation are implemented. |
| Events | Durable adapter operations, receipt reconciliation, event ingestion and recipient notifications over authenticated SSE |
| Platform | Public deployment-specific branding, currency display metadata, project types and terminology; software/Abako default, integer mock currency with decimals 0 |
| Development | Local Compose/Nginx at localhost:8088; real-service API E2E with disposable PostgreSQL in both mock modes |
| Delivery | Tag-triggered backend image workflow and an approved mock-backed HTTPS preview; see [cloud status](../operations/cloud.md) |

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

The mock is the current business source of truth. It validates signed custom
SCALE calls and runs Rust domain modules atomically. SQLite persists a serialized
state row containing business changes, nonces, receipts and events; memory mode
is disposable. Neither mode produces parachain blocks or executes contract WASM.

The mock-backed app has evolved beyond the deployed ink! v5 contracts. Sharing
that logic through a compatible contract runtime and implementing a real Kreivo
provider are future work, not existing deployment options. Changing an endpoint
URL cannot establish compatible encoding, execution or reconciliation. ink! v6
is outside the intended scope.

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
- Dependency-policy findings and production supply-chain gaps remain explicit in
  [supply-chain status](../security/supply-chain.md). Package metadata and the
  template license notice need an owner decision before being treated as a settled
  project licensing statement.

## Evidence and next work

The [platform handoff](../../progress/handoffs/PLATFORM-001.md) records 106 workspace
tests, doctests, formatting, check, Clippy, build and eight real-service scenarios
for the platform feature branch, with missing-tool and dependency-check limits.
It is historical evidence, not a claim that all gates pass on the latest checkout.

The [porting review](porting-coverage.md) compares the legacy implementation and
records older verification. It is not the current product roadmap. The approved
[repository simplification](repository-simplification.md) is the next cleanup;
the contract-runtime proposal remains separate future work.
