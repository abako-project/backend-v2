# SPEC-0013: Run canonical ink! v5 contracts in the local provider

Status: APPROVED.
Approval: owner accepted the staged design and requested implementation on
2026-10-21. This approves the embedded FRAME/pallet-contracts dependency family
for the compatibility proof; it does not authorize publication or deployment.
On 2026-10-22 the owner explicitly approved the reviewed four maintenance-advisory
and thirteen exact-version license exceptions for SDK provider-integration work
and requested continued implementation. The approved scope and review expiry
are in `tools/ink-sandbox/policy/`. This does not approve additional exceptions,
authority promotion, publication or deployment.

## Scope and invariants

Replace the mock's Rust business rules progressively with canonical ink! v5
WASM, preserving a fast local environment without a parachain node. Contracts
remain in `abako-project/contracts`; the backend pins immutable artifacts. v6
and a live Kreivo provider are excluded. Existing approved application behavior
is canonical; update contracts to match it rather than restore legacy behavior.

Keep public REST/JSON/SSE, sessions, CSRF, asynchronous operations, signed
`SignedContractCallV1` encoding and SCALE variant order unchanged. Preserve
accounts, public IDs, projects, amounts and historical records. Adapter and
wallet ownership remain unchanged. Contract deployment funding is separate from
user asset balances; do not scale amounts or silently impose new user fees.

The provider shell owns signature/expiry validation, nonces, idempotency,
receipts, event cursors and durable atomic commits. Contracts own domain
permission checks, catalogs, qualifications, calendars, selection, project
lifecycle, compensation, escrow, settlement, evaluations, disputes and Bramp
holds. A small ABI bridge supplies the verified user as the direct Project
caller; a factory/router must not become the payer of an asset deposit.

Use one embedded minimal FRAME runtime with contracts, timestamp, balances and
assets plus the required Kreivo asset extension. Meter WASM and host work. A
bounded dedicated worker owns execution; a Tokio timeout alone cannot cancel a
blocking interpreter. No node, network call, block wait or startup compilation
is needed. Checkpoints must include child tries, root, format/runtime version
and bundle hashes. Startup rejects incompatible state or artifacts.

Capture time, generated IDs and tie-selection input once per operation. Execute
against tentative state. Atomically commit business changes, nonce, receipt and
events. Domain rejections discard all business effects but commit their failed
receipt and nonce; infrastructure failure commits nothing. Decode runtime traps,
revert flags, ink language errors and domain results, including nested calls.
Read queries do not commit effects.

## Delivery sequence

Each row is independently reviewable and must preserve the working frontend.
The Rust engine remains authoritative until the whole candidate passes parity.

| Milestone | Deliverable and promotion gate | Estimate |
|---|---|---|
| 0 | Executable v5 WASM proof: cross-calls, Kreivo assets, caller semantics, rollback, metering and full restart snapshot; select a coherent SDK/toolchain | 3–5 days |
| 1 | Separate envelope shell and Rust engine, reproducible operation inputs, versioned checkpoints; legacy SQLite opens unchanged | 1 week |
| 2 | Embedded bounded worker, verified immutable bundles, atomic runtime persistence and private diagnostics; fault tests | 1–2 weeks |
| 3 | Directory and calendar contracts reproduce catalogs, registration, modes, ISO-week capacity and reservations | 2 weeks |
| 4 | Exact reputation and matching parity: skill intersections, prior-team preference, unbiased ties, no role filter/truncation | 1–2 weeks |
| 5 | Project factory/instances, unpaid planning, proposals, revisions, optional tasks and estimates | 2–3 weeks |
| 6 | Full execution/escrow, captured payout shares, sequential settlement, disputes, evaluations and Bramp parity | 2–3 weeks |
| 7 | Complete import, sealed migration authority and full shadow comparison; current-state rollback rehearsal | 1–2 weeks |
| 8 | Whole-provider switch, two stable release cycles retaining the Rust reference, then retire duplicate business rules | 1–2 weeks plus observation |

Partial contract domains run only against explicit fixtures/imported snapshots.
Missing coverage is reported as incomplete, never as a successful comparison.
`rust`, `shadow` and `sandbox` are whole-provider modes; do not split authority
by domain or automatically fall back/re-execute a failed sandbox command.
Default remains `rust` until the explicit promotion gate passes.

## Milestone review

Owner instruction, 2026-10-21: each milestone ends with an independent, in-depth
review of the exact implementation commit before it is marked complete or
proposed for release. Review compatibility, data preservation, authorization,
money, failure atomicity, verification evidence and unnecessary complexity as
applicable. When a PR spans milestones, give a separate verdict for each one.
Fix blocking findings and have the revised commit reviewed again. Record the
verdict and unresolved acceptance gates in the PR or handoff. Review does not
replace owner approval for releases or deployment.

## Cutover and rollback

Briefly pause new business writes, drain pending work and reconcile unknown
outcomes; abort on ambiguous operations. Import into fresh contracts, map public
IDs to addresses, retain provider generation, accounts, nonces, receipts,
timestamps and cursors. Do not replay payments, emit historical notifications,
reprice proposals or recompute reputation. Import privilege is permanently
sealed. Validate the complete export before switching authority.

For two release cycles maintain the Rust reference using identical operation
inputs. Rollback imports the **current** state, including sandbox writes, through
a compatible bridge release. Restoring a pre-cutover checkpoint would lose
accepted work and is forbidden.

## Acceptance

Run existing application tests unchanged. Add differential checks of IDs,
outcomes, revisions, assignments, reservations, financial totals and events.
Exercise invalid signatures, replay, stale nonce, domain failure, resource
exhaustion, restart and competing SQLite providers. Rehearse complete legacy
migration and rollback after new writes; verify contract authorization directly.
Run the public gateway smoke flow before authority promotion.

Milestone 0 fixes benchmark fixtures. Initial target on 4 vCPU/8 GB: warm ordinary
engine execution p95 <= 250 ms, four-milestone approval <= 1 s, startup <= 5 s.
Separate engine timing from transport polling. These are promotion budgets, not
claims about unmeasured builds. Deterministic tie input is for reproducibility,
not a guarantee of secure onchain randomness.

No new product rules are approved here. Unknown migration rules or runtime
incompatibilities block the affected milestone and must be recorded with the
reproduction command. Later milestones depend on the compatibility proof.

Implementation evidence and the current integration gate are recorded in
[the first-slice handoff](../../progress/handoffs/SPEC-0013.md).

Milestone 2's isolated worker/persistence slice and its remaining shell/policy
gates are tracked in [the candidate handoff](../../progress/handoffs/SPEC-0013-M2.md).
This progress record does not change the approved promotion requirements.

M2 infrastructure and real-shell fixture integration are complete at reviewed
implementation `65b9531`, with full application verification passing. See
[the integration evidence](../../progress/handoffs/SPEC-0013-M2-SHELL.md). This
does not complete domain parity, M0 performance acceptance or authority promotion.


M3 Calendar/Directory and signed fixture integration are complete at independently
reviewed implementation `9dccb98`. The real-WASM suite (40 tests), unchanged-policy
SDK dependency checks and full application gate (112 tests, 2 doctests, 8 E2E flows)
pass. See [M3 evidence](../../progress/handoffs/SPEC-0013-M3-SHELL.md). This closes
M3 within its explicit fixture scope; it does not complete M4 matching, actual
Project hooks, import/rollback, performance acceptance or authority promotion.


M4 exact reputation/indexed matching parity is independently approved at backend
`0ca0cf1` and canonical `192279d`, within explicit fixtures. All 44 SDK tests,
SDK policy checks, fresh-source WASM/ABI reproduction and the full app gate
(113 tests, 2 doctests, 8 E2E flows) pass. [M4 evidence](../../progress/handoffs/SPEC-0013-M4.md)
records the inherited version-1 sampler's minute documented bias; this preserves
native behavior and does not claim strict mathematical unbiasedness. No retained
input semantics are silently changed. Actual Project composition is M5/M6.
