# SPEC-0014: Keep evidence records offchain

Status: DRAFT — commitment direction approved; anchoring scope pending.
Approval source: owner accepted adapter-side validation, fixed-size onchain
hashes of immutable evidence records, preserved signed-command compatibility
and unchanged frontend requests/responses in the current conversation.

## Problem and decision

The sandbox candidate reproduces legacy HTTPS validation through a separate
`references-v5` WASM contract because the full parser exceeded Project's code
limit. Current application flows already store descriptions and written
arguments in adapter PostgreSQL, referencing their IDs from the provider.

Keep complete records in the adapter. Store a fixed 32-byte commitment in the
provider/contracts, bound to the existing business operation and record ID.
Remove full URL parsing and the References contract from current execution.
Contracts keep permissions, lifecycle checks, replay handling and money rules.
They do not interpret or fetch external references.

## Requirements

- Hash an immutable, versioned evidence record, using the application's existing
  SHA-256 implementation and an explicit deterministic encoding. Include its
  kind, provider generation, business target, record ID, author and exact payload.
  Specify fixed test vectors before integrating either execution engine.
- Persist the full record before queuing its signed commitment. Repeated IDs
  must resolve to the identical record. Conflicting content is rejected.
- Validate input in the adapter according to its existing contract. Legacy HTTPS
  references keep their validation there; current descriptive text retains its
  accepted protocols, length bounds and visibility. Do not introduce an HTTPS
  restriction on arbitrary written arguments or delivery descriptions.
- Sign the commitment and target together. A caller-supplied hash does not prove
  any URL or document is valid. Contract authorization remains based on the
  verified caller and current project/submission/dispute state.
- Verify adapter records against confirmed commitments when assembling the
  corresponding read model. Missing/mismatched data is explicit; it never
  silently substitutes another record or changes business state.
- Preserve public REST/JSON/SSE requests, response shapes and access controls.
  No frontend signing, content-upload service or new process is introduced.
- Add versioned internal commands/fields. Do not repurpose the legacy digest
  slot, reorder SCALE variants, reinterpret old signatures, or fabricate
  historical commitments where their original offchain content is unavailable.
- Preserve complete historical records through native/sandbox import, export,
  restart and current-state rollback. Adapter evidence remains adapter-owned;
  the provider never reads PostgreSQL directly.
- Record commitments in the same atomic operation as their business effect.
  Rejected operations do not publish the tentative record as confirmed.
  Infrastructure failure commits neither business changes nor commitments.

## Meaning and limits

A commitment proves that supplied bytes match a recorded hash. It cannot
recover missing content or make an external website immutable. Hashing a record
containing a URL does not hash the remote document. Adapter retention/backups
remain necessary. Fetching, snapshotting or content-addressed document hosting
is outside this change.

## Scope decision required before implementation

Current rejection comments and dispute-opening arguments exist before their
signed operation. Delivery documentation/links are saved after confirmation of
the submission; later public arguments also have no existing signed operation.
Their immutability and lack of provider writes are covered by current tests.

Choose whether this change anchors:

1. Evidence already referenced by signed operations, including rejection and
   dispute-opening records; or
2. Those records plus delivery descriptions and later public arguments, requiring
   additional anchoring operations and explicit confirmation/failure semantics.

Do not add extra transactions or claim unsigned descriptive records are anchored
until that scope is decided. Existing private case messages remain offchain.

## Delivery and checks

1. Specify the chosen record scope, encoding, vectors and legacy bridge; add
   compatible native commands and adapter persistence/read verification.
2. Implement the same commitments in canonical v5 contracts, remove References
   wiring, rebuild/pin artifacts, and preserve complete migration/rollback.
3. Run application and isolated SDK gates, native/WASM differential and gateway
   tests, old-signature/old-archive regressions, duplicate/conflicting record
   checks, missing/tampered record reads and failure/restart atomicity tests.
4. Obtain independent Astra xhigh review of the exact candidate changes and
   update the consolidated PRs with separate implementation/evidence commits.

No release, authority promotion, public breaking change or destructive migration
is included. Broader legacy fixture/API packaging cleanup is separate.
