# R2b1: proposal content ownership

Implementation slice of approved SPEC-0015 R2. Catalog names/moderation and
cancellation/dispute reason text are the following R2b2 slice; task ownership is
R3. This slice must finish as a working native/WASM/API cohort, with exact-state
rollback, before acceptance. No release, live migration or deployment is implied.

The adapter owns exact proposal titles/descriptions, milestone titles and the
latest change-request explanation in one immutable `proposal_documents` context.
Definition and change request have independent fixed references; editing a
proposal does not erase a previous request. Existing presentation/comments remain
in their present context tables. API staging and the queued compact command commit
in the existing operation transaction, retaining the original public request for
idempotency. No extra service, generic document store, dependency or public limit.

The signed document binds generation, Project, operation, verified author and
explicit target proposal (unknown until creation succeeds). It contains the exact
original definition, including numeric quoted inputs, or exact change-request
text. Migration identifies the source file hash and retained proposal without
inventing a signer. Core receives real numeric milestone terms and a fixed
operation/hash commitment, never descriptive text. The core still checks keys,
qualifications, amounts, windows, permissions, lifecycle and captured allocation.

Create assigns revision 0. Update retains its existing permission/order behavior
and captures the compensation policy during execution. The API never predicts
revision/policy or allocates payouts from its snapshot. Definition refs and actual
terms/policy are committed atomically. SetDelivery, Submit and Withdraw may advance
revision while retaining the selected immutable document. Reads verify context,
digest, definition keys and input terms reallocated under the captured policy;
migrated terms are already allocated and compare directly. Mutable tasks,
assignments, submissions and milestone status are not part of that comparison.
Public views continue to use the core's current facts and revision.

Legacy full-text calls lower only after replay admission and retain their exact
recorded bytes, signatures, inputs and outcomes. A working V2 predecessor bridge
must decode/replay compact commands 51–53 but reject fresh execution without a
receipt/nonce change. Freeze that executable before the V3 cutover. Conversion is
explicit from that immediate predecessor, through owner exports and fresh stores;
current-state rollback rehydrates V2 through API-owned content, including successful
compact writes. Keep selected and historical content, and reject missing,
conflicting, foreign or corrupt records. Reconcile pending/ambiguous work before
migration; never rewrite signed commands. Core money/lifecycle operations and
standalone task reads must not depend on unrelated descriptive content.

Final acceptance: unchanged public DTOs/REST/SSE and IDs, exact native/WASM
financial and history parity, revision/policy races, authorization and missing or
corrupt content, PostgreSQL transactions/recovery, sealed imports, restart,
failed-commit recovery, and real frozen-predecessor rollback after successful
compact create/update/change request and further ordinary work. Run the app/SDK
and unchanged SDK policy gates; rebuild changed v5 source artifacts. Astra xhigh
reviews the exact milestone. Do not stop at the decoder bridge as the deliverable.


## Current encoding and storage

CoreSnapshotV3 has `schemaVersion: 3`; CoreArchiveV3 has `version: 3` and
`engine_version: 3`. ProposalCore holds actual numeric terms, revision, captured
compensation policy, assignments, task storage, status and two independent
content references. Public DTOs remain unchanged. `proposal_documents` is an
additive PostgreSQL table; JSON text preserves exact Unicode and NUL. R1 child
copies now contain tasks only. Historical rows are retained.

The SHA-256 preimage is literal `abako/proposal-document/v1`, one NUL byte, then
SCALE generation, Project ID, provenance and content. Operation provenance binds
operation ID, verified author and optional target proposal (None for creation).
Migration provenance binds the exact source archive SHA-256 and retained proposal.
Definition content contains the original quoted inputs and narrative; change
content contains the exact explanation. The data-only codec and five independent
vectors are in `proposal-metadata-vectors.json`. Signed references are 49 bytes;
Migration references are 33 bytes. Ordinary execution receives only the fixed
48-byte commitment and numeric terms; migration refs enter through sealed import.
No URL parser, descriptive proposal text or API document store is added onchain.

## Paired V2 to V3 cutover

The immediate predecessor is the reviewed decoder bridge
`38287daa651fa617886ce08342facff4ffab0815`. Freeze its adapter, wallet, native and
whole-provider binaries together. It retains R2a creation references and decodes
commands 51–53 for replay, rejecting their fresh execution. Earlier V1 installations
must first follow the historical [R2a transition](r2a-creation.md); this binary
converts only V2 to V3 and runs one current engine.

Stop public writes and background operation workers. Using the matching
PostgreSQL and original keys, reconcile pending/ambiguous work and run:

```sh
adapter-api check-provider-cutover
```

This rejects nonterminal operations and Expired rows marked possibly submitted.
Stop every other writer. Back up PostgreSQL, provider/wallet SQLite, matching
secrets/configuration and the frozen executables as a coherent cohort. Export the
stopped predecessor's current provider state, retaining its exact file bytes:

```sh
old-whole-provider export-native predecessor-v2.json
# Use export for a predecessor whole-provider store.
whole-provider upgrade predecessor-v2.json core-v3.json proposals.ndjson
```

Use existing root/configuration. Conversion validates history, retains IDs,
RNG/task counters, money, nonces, signed bytes, receipts, inputs and events, and
emits explicit migration documents from already allocated V2 terms. It preserves
all R2a worker/Project creation refs. Outputs must be new files; Unix permissions
are 0600. A partial multi-file export is an incomplete cohort: retain sources and
retry with fresh destinations.

Import into a fresh provider store, start it internally and keep public writes
stopped while verifying the matching adapter data:

```sh
mock-provider import core-v3.json fresh-native.sqlite
# Or configure a fresh WHOLE_DATABASE_PATH and the chosen authority:
whole-provider import core-v3.json
adapter-api import-proposal-metadata proposals.ndjson
adapter-api backfill-provider-content
```

Proposal NDJSON uses a version 1/generation header and typed immutable records.
Import verifies selected context, digest, kind and actual numeric terms; it is
atomic and idempotent. Missing/duplicate/foreign/conflicting content fails.
Partial recovery files are accepted only when every other selected record already
exists and verifies. Parsing is sequential with the existing 32 MiB line bound;
the offline converter also retains its 64 MiB total archive/metadata bound. No
service reads another service's database. Resume only after coherent target and
public API checks pass.

## Current-state rollback and recovery

Pause and reconcile again. Export selected proposal content from the current API
while its provider remains internally available, then stop the provider:

```sh
adapter-api export-proposal-metadata current-proposals.ndjson
whole-provider export current-v3.json
# Use export-native for native SQLite.
whole-provider downgrade current-v3.json current-proposals.ndjson current-v2.json
```

Downgrade uses current facts and verified documents, including successful compact
create/update/change calls. It rehydrates only proposal text, milestone titles and
change explanations. Creation refs remain unchanged; do not replace them with
legacy text or convert them back to V1. Import `current-v2.json` into a fresh
store using frozen 38287da, and restore that paired adapter/provider with the same
wallet, PostgreSQL and keys. The predecessor adapter leaves the additive proposal
table and data intact. Verify reads, exact replay after restart and new ordinary
work before resuming. A pre-cutover checkpoint would lose later work.

API backup remains necessary for profiles, comments, tasks, evidence, sessions,
pending requests and historical documents; the exchange contains only selected
confirmed proposal content. Compact calls contain hashes only. Successful direct
legacy calls can recover selected preimages through:

```sh
whole-provider recover-proposal-metadata current-v3.json recovered-proposals.ndjson
adapter-api import-proposal-metadata recovered-proposals.ndjson
# R2a creation recovery remains available separately:
whole-provider recover-creation-metadata current-v3.json recovered-creation.ndjson
adapter-api import-creation-metadata recovered-creation.ndjson
```

Recovery verifies successful retained receipts and selected refs, excluding
rejected and superseded calls. It cannot reconstruct compact preimages without
API backup. All existing provider/cell/archive/memory limits stay in force;
conversion fails if the target cannot fit.

The disposable real-WASM test emits an actual successful-write rollback case:

```sh
R2B_ROLLBACK_OUTPUT_DIR=/tmp/r2b-case cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked --test proposal_metadata -- --nocapture
python3 scripts/check-core-rollback.py --case /tmp/r2b-case --candidate-binaries /path/to/current-binaries --predecessor-binaries /path/to/38287da-binaries
```

The rehearsal uses disposable stores and known test identities. It exercises all
three predecessor authorities, their actual paired adapter, populated proposal
PostgreSQL, replay/restart and further ordinary REST work. It is evidence for
this development transition, not a live migration or production recovery promise.
