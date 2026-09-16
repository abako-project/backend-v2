# SPEC-0004: Auxiliary Virto compatibility

Status: DRAFT — implementation plan requested on 2026-09-15, not yet approved.

## Goal and boundary

Restore the useful auxiliary interfaces of the legacy backend without restoring
its permissive authorization or success-only stubs. Keep the public REST/SSE API
independent of both frontends. No new dependency or service is approved by this plan.

The first delivery should cover membership, governance remark submission and Bramp
simulation. Full DAO voting and real fiat settlement are separate decisions, not
capabilities already present in the legacy. Existing marketplace rules stay intact.

## Evidence and missing compatibility

Baseline: legacy `main` merge `3e2b929`, which contains task-storage implementation
`d408641`. Paths below are relative to that repository. This inventory describes
local source, not verified compatibility with a current external Virto deployment.

| Surface | Legacy source and operations | Rust status / proposed disposition |
|---|---|---|
| Membership | `packages/mock-api/src/virto-mock.ts`: community address, paginated members, lookup/check, add/remove; also global `add-member` and `is-member` | Missing. Add one community-scoped membership model; explicitly map or retire the global aliases |
| Governance | Same file: `POST /memberships/governance/submit-remark`; `packages/adapter-api/src/modules/dao/dao.service.ts` requests call data then signs it | Missing. Legacy returns zero-filled call data, not a referendum lifecycle. Implement a recorded, authorized signed remark operation first |
| Bramp | `packages/mock-api/src/contracts-mock.ts`: create/list users, request/confirm deposit, request withdrawal; adapter `modules/ramps` | Missing. Legacy does not store deposit/withdrawal lifecycle or settle their balance effects. Implement stateful simulation, not unconditional success |
| Virto identity/authentication | `virto-mock.ts`: attestation, register, assertion, chain-head, password-register/connect/change-password, user/address lookup | Wire compatibility missing; classic auth and custody already replace the password-derived-key model. Do not restore mock JWTs or claim passkey support |
| Kreivo JSON-RPC | `packages/mock-api/src/kreivo-mock.ts`: `chain_getHeader`, `chain_getFinalizedHead`, `state_getStorage`, batches | Missing; legacy serves fixed account/asset bytes. Only add a narrow fixture if an identified consumer requires it, not a fake general-purpose node |
| Generic payments | `virto-mock.ts` and `packages/mock-api/src/payments.ts`: create/release/get/request/accept-and-pay/refund/cancel/dispute/resolve | Missing as standalone APIs. Marketplace escrow is implemented. Generic payment resolution requires a separately approved money/authorization specification |
| Contract wrappers | `contracts-mock.ts`: project/calendar constructors, deploy/query/call and ratings deployment | Wire compatibility missing. Domain equivalents exist behind signed provider commands; map only methods needed by identified consumers, not a second business implementation |

Catalog and worker qualification routes also differ at the wire level, but their
domain behavior is already implemented. Rich profiles, role-skill associations,
sequential activation, assignment continuity and explicit project completion are
product differences listed in `docs/project/porting-coverage.md`, not missing Virto
infrastructure mocks. This plan does not silently restore those semantics.

## Proposed implementation

1. Freeze a compatibility matrix with the other frontend team: caller, method,
   request, response, errors and whether an old alias is actually needed. Commit
   sanitized fixtures from the legacy. New public endpoints use `/api`; private
   compatibility endpoints are not exposed through Nginx. Verify external Virto
   contracts against a selected release before claiming external compatibility.
2. Define context-owned IDs and wire types: community/member/remark IDs;
   `Membership`, `GovernanceRemark`, `RampUser`, `Deposit`, `Withdrawal`. Reuse
   validated account, amount, operation and receipt types. Link a ramp user to its
   authenticated owner; do not take authority from a supplied email or account.
3. Add membership and remark modules to the existing transactional mock. Stable
   community addresses, membership changes, receipts and events commit atomically
   on both memory and SQLite. The verified origin authorizes writes. Community
   membership must not automatically grant marketplace coordinator eligibility.
4. Add Bramp state to the mock, with fixture bank details explicitly marked fake.
   Proposed deposits: Pending -> Confirmed or Failed. Proposed withdrawals:
   Pending -> Settled or Failed. Deposits credit once only on trusted confirmation;
   withdrawals reserve available funds on request and settle or release that
   reservation once. The account/amount/destination cannot change on confirmation.
   Any internal retry, including a new transport operation for the same deposit,
   must not credit/debit twice. Reuse the provider ledger, never a second balance
   in the adapter. Agree the state transitions and asset rules before coding.
5. Add typed adapter endpoints and operation/SSE mapping using the existing
   authenticated session -> custody signature -> provider receipt flow. Never
   accept arbitrary signing bytes from the browser or fake an extrinsic hash as
   evidence of real blockchain inclusion. Expose queryable operation status.
6. Add independent UI pages for membership, submitted remarks and ramp operations
   after the shared public contract is frozen. Both frontends use the same API;
   backend completion must not depend on Leptos.

The mock is the business source of truth; custody remains the only seed owner.
No standalone Bramp container is needed for this simulated slice. For a future
external ramp, its settlement reference becomes an authenticated input to the
chain/provider ledger. A webhook is useful for that asynchronous confirmation,
not for bypassing signatures or making the browser authoritative. Replay protection
and provider-specific callback authentication must be specified before that integration.

A future real provider will need encoding, runtime metadata, account mapping,
submission and finality handling. Replacing a URL alone does not supply those.

## Task graph and ownership

Each implementation writer gets one task branch/worktree. These are future
assignments, not worktrees to create now. Shared changes belong to the integrator.

| Task | Owner / write scope | Depends on | Done when |
|---|---|---|---|
| VIR-00 | Integrator: this spec, ADR and compatibility fixtures | Product answers below | Scope, permissions, money rules and acceptance approved |
| VIR-01 | Contracts agent: generated-contracts, OpenAPI via integrator | VIR-00 | DTOs, errors, examples and signed message mapping reviewed |
| VIR-02 | Membership/governance agent: dedicated mock modules/tests | VIR-01 | Authorized add/remove/query and recorded remark, identical storage behavior |
| VIR-03 | Bramp agent: dedicated mock module/tests | VIR-01 | Deposit/withdrawal lifecycle, conservation and replay tests pass |
| VIR-04 | Integrator: provider dispatch/state/migrations and ledger integration | VIR-02, VIR-03 | One atomic domain commit; no competing state or schema writers |
| VIR-05 | Adapter agent: dedicated handlers/tests | VIR-01; integrated tests after VIR-04 | Sessions, owner checks, operation polling and SSE pass |
| VIR-06 | Frontend agent: Leptos pages/tests | VIR-01; integrated tests after VIR-05 | No access to custody/provider; ordinary REST client works too |
| VIR-07 | Independent verifier: E2E and verification handoff | VIR-04, VIR-05 | Both storage backends pass success, denial, replay and failure cases |

After contracts freeze, VIR-02, VIR-03 and VIR-05 can proceed in parallel in
disjoint files. The integrator alone changes shared dispatch/state. VIR-06 can
use contract fixtures meanwhile. Do not start implementation from this draft.

If full DAO governance is approved, add a separate GOV-02 task after VIR-02:
proposal creation, voter eligibility snapshots, votes, closing/tallying and execution
of an allowlisted action exactly once. Its contract must define rejected/expired
proposals, changes in membership during voting, tie handling and failed execution.
Tests must prove that voting cannot directly bypass system-only marketplace writes
or release disputed escrow. No voting defaults are inferred from the remark stub.

## Required acceptance evidence

- Membership: stable address across reads; consistent paginated list/check/get;
  duplicate add/replayed remove is safe; unauthorized changes and cross-community
  access follow the approved policy; no automatic coordinator promotion.
- Remark: payload and origin are bound to the signature; pending/finalized status
  and one receipt/event are observable; rejection does not claim a created vote.
- Deposit: request changes no spendable balance; trusted confirmation credits the
  original owner/amount once; wrong owner, unknown ID, changed destination and
  repeated confirmation cannot mint extra funds.
- Withdrawal: insufficient funds leaves state unchanged; concurrent requests
  cannot overspend; success consumes the reservation once and failure restores it
  once; disputed marketplace escrow is never withdrawable.
- HTTP: lost successful responses, duplicate/out-of-order confirmation and invalid
  callbacks cannot double-settle; browser clients cannot confirm external payment.
- Run identical scenarios with memory and SQLite, including rollback on failure.
  No real funds, banking data, private keys or production credentials in fixtures.

## Decisions needed before implementation

Recommended first slice, subject to approval:

1. One seeded mock community; system authority adds/removes members, authenticated
   members read its directory and submit informational remarks. No vote, quorum,
   treasury action or automatic change to marketplace parameters yet. If real
   governance is needed now, define electorate, voting weight, threshold, duration,
   execution authority and actions before adding those states.
2. Bramp uses mock asset 1 (KVN) in integer units, no real fiat conversion or fees.
   Only system authority simulates confirmation/failure; users operate their own
   ramp account. Approve reservation/release behavior above and whether withdrawals
   to destinations other than the custodial account belong in this POC.
3. Prioritize membership/remarks and Bramp; keep WebAuthn/password-derived keys,
   general RPC, generic payment arbitration and unused legacy wrapper aliases out
   until a named consumer requires them. Full custodial login remains unchanged.

No DAO pricing or dispute-resolution authority is implied by adding a governance
endpoint. Those would amend SPEC-0003 and require explicit approval.
