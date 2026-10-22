# SPEC-0014: Keep evidence records offchain

Status: APPROVED.
Approval: owner accepted the commitment direction and then the recommended
existing-signed-operation scope in the current conversation, 2026-10-22.

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

## Anchoring scope

Anchor evidence referenced by existing signed operations: optional URL delivery
references, rejection reasons, dispute openings and the retained legacy response
operation. Current rejection/opening texts keep their existing scoped IDs.
Delivery descriptions, later public arguments and private case messages remain
offchain; they acquire no additional transactions or new protocol restrictions.

Append four internal commitment commands. Existing evidence read/storage slots
hold either a legacy reference or a commitment; no second catalog owns evidence
state. The adapter resolves commitments after project visibility filtering or
for the requested public case. Raw authorization snapshots never hydrate all
records. Missing evidence cannot block unrelated projects or authorization.
Both untagged reference shapes reject unknown fields; unresolved internal
commitments must never reach a frontend response.

Text stays in the existing comment/opening tables. New URL records use an
adapter table keyed by generation, project, purpose, business target and record
ID. Existing scoped comment IDs do not become globally unique. URL record
persistence and pending-operation insertion share one adapter transaction after
queue admission. Preserve original request bytes for queue-level idempotent retries after the
signed-command conversion. Existing endpoint target/current-state checks remain;
this change does not broaden HTTP retry behavior after target removal or reset.
Persist the evidence generation in its pending operation; a changed provider
generation must reject before signing rather than rebind the record hash.

## EvidenceRecordV1 encoding

Compute `SHA256(b"abako/evidence/v1\0" || SCALE(record))`. In order, `record`
contains version `u8=1`, provider generation `[u8;16]`, project `[u8;16]`, purpose
`u8`, target `[u8;16]`, record ID `[u8;16]`, author `[u8;32]`, then body kind `u8`
and the exact UTF-8 payload as a SCALE string. Do not normalize URLs, whitespace
or Unicode before hashing. The domain prefix ends with one NUL byte.

Purpose tags and targets are: delivery `0`/milestone ID, rejection `1`/submission
ID, opening `2`/rejected submission ID, response `3`/dispute ID. Body tags are URL
`0` and text `1`. Rejection/opening text retains its existing comment ID; new
URL records use their operation ID's 16 bytes. The provider assigns any new
submission/dispute ID within the same operation as storing the commitment.

Frozen record and legacy-reference byte/hash vectors are in
[hash-vectors.json](hash-vectors.json). The adapter uses existing SHA-256 and
SCALE implementations; contracts accept fixed hashes without recomputing or
validating their offchain preimages.

Preserve old native read/storage SCALE encoding for legacy references. A new
committed reference uses bytes `ff 01`, record ID `[u8;16]` and hash `[u8;32]`.
A valid legacy URL's compact length (<=2048 bytes) cannot start with `ff`.
This discriminator is confined to provider storage/read codecs; signed request
reference types and every old signed-command byte remain unchanged. The new
contract storage codec is explicit and converted by the private SDK bridge.

## Legacy compatibility

Retain old signed-command bytes and their native semantics. The verified SDK
bridge replaces each legacy URL reference at the contract ABI boundary with
`SHA256(b"abako/legacy-reference/v1\0" || SCALE(EvidenceReference))`, including
its opaque legacy digest slot. This narrow compatibility hash commits exactly
the old reference bytes; context remains bound by its signed operation and
contract storage slot. It does not invent a record ID or adapter content.

Retain a bounded offchain compatibility map in the candidate checkpoint for
lossless old reads, complete export and rollback. Contracts store only the fixed
legacy-reference hash. Import stages the same hashes and retains original bytes
offchain; it does not replay historical operations. A missing or mismatched map
entry is an infrastructure failure, never an empty or substituted reference.

Old comment-ID-only history remains explicitly unanchored. Absence is optional
metadata, not an all-zero hash. Never derive a historical text hash from whatever
the adapter happens to hold now.

New adapter records use the full context-bound EvidenceRecordV1 hash; the legacy
exception does not apply to new frontend requests. Old commands accepted through
the compatibility bridge may still be replayed unchanged.

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
