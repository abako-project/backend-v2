# Capabilities and limits

This describes the current source. Verification belongs to the revision and
commands recorded in the [backend review](https://github.com/abako-project/backend-v2/pull/24)
and [paired contracts review](https://github.com/abako-dev/contracts/pull/21).
Implemented behavior is not a claim of deployment or production readiness.

## Implemented

| Area | Behavior |
|---|---|
| Identity | Password and username-first passkey login, sessions, CSRF and custodial sr25519 signing |
| Marketplace | Qualifications, skill matching, coordinator selection, proposal review, assignments and sequential milestones |
| Capacity | Integer minutes and ISO-week windows; all milestone assignments reserved on execution approval |
| Payments | No separate planning fee; full execution escrow upfront and stored exact shares paid per accepted milestone |
| Compensation | Configurable coordinator percentage, initially 10%; checked integer allocations based on planned minutes |
| Tasks | Core IDs/allocator/assignees with API-owned text/progress; task tracking does not determine payouts |
| Content | API-owned profiles, catalog names, creation/proposal documents, evidence, comments and lifecycle reasons |
| Evaluations | Final 1–5 ratings and minute-weighted reputation, separate from payments |
| Disputes | Opening on the current rejection, project freeze, public arguments and a private case conversation |
| Delivery | Durable operations, idempotent replay, uncertain-outcome recovery and recipient notifications/SSE |
| Platform | One configurable DAO profile per deployment: branding, vocabulary, project types and currency display |
| Providers | One embedded ink runtime with one WASM authority |
| Recovery | Fresh development initialization, atomic persistence and restart |

The API, wallet and provider remain separate services. Both application
contracts use ordinary ink storage; no custom paging helper or onchain URL parser
is used. Metadata and task ownership are described in [architecture](architecture.md)
and the [requirements](../specs/README.md).

## Not delivered

- Real-chain submission, DAO-configured payment/governance assets, real assets,
  bank integration, DAO governance, tenant isolation, currency conversion or
  generic external payments. Bramp currently
  models deposits and withdrawal holds with operator confirmation.
- Formal dispute rulings, resolution payments, refunds or unfreezing. Private
  conversation access remains contractual; public arguments remain client/coordinator-only.
- Per-milestone delivery dates or time-extension requests. Current target dates
  are advisory and do not trigger penalties or payments.
- Production custody guarantees such as HSM integration, online key rotation or
  an established production recovery/retention policy.

## Release gates

The contract provider is the only development execution engine. Release requires
the current dependency-policy check; controlled-host performance acceptance
and frontend deployment verification remain open. Development starts with
clean matching data; no historical migration is provided.

The isolated SDK policy includes an owner-approved development-only exception
for RUSTSEC-2026-0387 in crypto-bigint, expiring 2026-12-01. The vulnerable
dependency is unchanged; see the [scope and exposure review](../services/contract-provider/policy/README.md).
Application dependency findings and the unresolved project-license notice remain
in [supply-chain status](security/supply-chain.md). Functional tests do not waive them.

The independent marketplace frontend's task-result/notification compatibility was
prepared in [PR 55](https://github.com/abako-project/abako-web/pull/55). Source review
is not evidence of a deployed browser cutover. The [preview guide](operations/digesta-backend-preview.md)
describes the existing mock-backed environment; source changes do not deploy it.
