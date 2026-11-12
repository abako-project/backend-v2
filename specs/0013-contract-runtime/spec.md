# SPEC-0013: Run canonical ink! v5 contracts in the local provider

Status: APPROVED.
Approval: owner accepted the staged design and requested implementation on
2026-10-21. This approves the embedded FRAME/pallet-contracts dependency family
for the compatibility proof; it does not authorize publication or deployment.
On 2026-10-22 the owner explicitly approved the reviewed four maintenance-advisory
and thirteen exact-version license exceptions for SDK provider-integration work
and requested continued implementation. The approved scope and review expiry
are in `services/contract-provider/policy/`. This does not approve additional exceptions,
authority promotion, publication or deployment.

On 2026-10-28 the owner additionally approved the exact-lock, expiring development
provider/image exception for RUSTSEC-2026-0387 documented in the SDK policy guide.
It covers the release-verification check, not publication/deployment or real-chain
use; no other vulnerability exception is approved.

The owner-approved fresh-install amendment in SPEC-0015 supersedes the historical
migration, maintained-native-comparison and two-release rollback requirements below.
Public API behavior and current execution safety requirements remain in force.

## Scope and invariants

Replace the mock's Rust business rules progressively with canonical ink! v5
WASM, preserving a fast local environment without a parachain node. Contracts
remain in `abako-dev/contracts`; the backend pins immutable artifacts. v6
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

## Authority

Partial contract domains run only against explicit fixtures/imported snapshots.
Missing coverage is reported as incomplete, never as a successful comparison.
`rust`, `shadow` and `sandbox` are whole-provider modes; do not split authority
by domain or automatically fall back/re-execute a failed sandbox command.
Default remains `rust` until the explicit promotion gate passes.

## Independent review

Owner instruction, 2026-10-21: each milestone ends with an independent, in-depth
review of the exact implementation commit before it is marked complete or
proposed for release. Review compatibility, data preservation, authorization,
money, failure atomicity, verification evidence and unnecessary complexity as
applicable. When a PR spans milestones, give a separate verdict for each one.
Fix blocking findings and have the revised commit reviewed again. Record the
verdict and unresolved acceptance gates in the PR. Review does not
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


The inherited version-1 sampler preserves native behavior, including its documented
minute bias; parity does not establish strict mathematical unbiasedness. Changing
that signed/replayed input behavior requires a separate approved change.
Current ownership requirements are refined by SPEC-0015. Verification results and
remaining promotion gates belong in the review PR, not a milestone transcript.
