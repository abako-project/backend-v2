# R2a: immutable creation content

Historical V1→V2 procedure, using the R2a binaries and tools. The current
V2→V3 proposal cohort uses [R2b1](r2b-proposals.md); its converter does not
convert V1.

Implementation of the approved SPEC-0015 worker-name and Project root
text slice. Public REST/JSON/SSE requests and responses are unchanged. Proposal,
catalog and task ownership remain later stages. Review/evidence is in the
[handoff](../../progress/handoffs/SPEC-0015.md).

## Authority and execution

The API owns immutable worker directory names and Project titles/descriptions in
`worker_directory_metadata` and `project_creation_metadata`. Names remain distinct
from editable identity/profile names. JSON text preserves accepted NUL and exact
Unicode. Metadata and the queued compact command commit in the existing enqueue
transaction after authorization, retry checks and queue admission. Original
request bytes remain available for idempotency. The existing operation ID binds
the record; creation consumes no extra random ID. Generation is fixed before
signing, so a provider reset cannot silently reinterpret the staged record.

New requests sign compact commands 49/50. WorkerCore and ProjectCore contain fixed
Creation/Migration references; neither stores these descriptive preimages. The
provider's private CoreSnapshotV2 has `schemaVersion: 2`, and CoreArchiveV2 has
`version: 2` / `engine_version: 2`. No empty substitute strings or hidden public DTO flags
are used. API public reads resolve and verify the selected record after applying
the existing visibility policy. Current proposal/task children remain core facts.
Raw authorization, money operations, command aliases and private case messages do
not depend on creation text. Optional notification titles degrade on missing
content; affected Project/worker presentation fails explicitly.

Signed legacy creation commands still validate and replay their original bytes.
After replay admission, a narrow converter derives the identical creation record
from generation, verified origin, operation ID and exact old content, then calls
the single compact handler. RecordedCall bytes are untouched. New Creation refs
must match a successful retained creation receipt and its entity/owner at archive
validation. A provider content cache or second old engine is not retained.

## Fixed private encoding

SHA-256 commits literal `abako/creation-metadata/v1`, one NUL byte, then SCALE:

1. Generation: 16 bytes.
2. Provenance: discriminant 0 Creation, operation ID (16 bytes) and author (32 bytes); or
   discriminant 1 Migration, source archive hash (32 bytes) and entity. Entity
   discriminant 0 is worker account (32 bytes); discriminant 1 is Project ID (16 bytes).
3. Content: discriminant 0 WorkerName plus its exact SCALE UTF-8 string; or
   discriminant 1 ProjectText plus exact title and description strings.

The domain versions the encoding. Whitespace, control characters and canonically
equivalent Unicode stay distinct. The four independent vectors are in
`creation-metadata-vectors.json`. Existing SHA-256 is reused; the data-only codec
adds no hashing/signing dependency. Large encoding/decoding runs on blocking
workers at async boundaries. A Creation reference is 49 bytes; Migration is 33.
Ordinary contract creation accepts only a Creation commitment (48 bytes).
Migration references enter only through fresh sealed import.

## Explicit immediate-predecessor transition

The supported predecessor is the reviewed decoder/replay bridge `84ab1e4`. It knows
commands 49/50 but rejects their fresh execution. Use a coherent backup of adapter
PostgreSQL, wallet/provider SQLite and matching secrets. No live migration or
release is authorized by this implementation.

Stop API writes and its background operation workers; reconcile every pending or
ambiguous operation through the existing receipt workflow. The new adapter CLI
can check the old PostgreSQL operation queue without reading an old snapshot:

```sh
adapter-api check-provider-cutover
```

It rejects nonterminal operations and an Expired row marked possibly submitted.
Do not rewrite signed bytes or manually mark an uncertain result expired. Stop
all other writers as well. With the predecessor's configuration/root, export its
current state through its own tool (`export-native` for native SQLite, `export`
for its whole-provider store). Preserve the exact file unchanged:

```sh
# Frozen predecessor tool; WHOLE_DATABASE_PATH selects its source store.
old-whole-provider export-native old-v1.json
# Current tool. Both destinations must be new protected files.
whole-provider upgrade old-v1.json core-v2.json creation.ndjson
```

Upgrade validates V1 / `engine_version: 1` and the resulting core state. It retains all
IDs, money, counters, timestamps and exact call/receipt/event history. Each moved
record explicitly identifies its retained entity and SHA-256 of the **exact old
file bytes**, including original whitespace. It does not claim an old signed
creation or invent its author. Reruns reuse that same immutable source file.

Import the core into a fresh destination through its owner:

```sh
mock-provider import core-v2.json fresh-native.sqlite
# Or configure WHOLE_DATABASE_PATH to a new file and the desired authority:
whole-provider import core-v2.json
```

Start the prepared provider internally while keeping the public adapter stopped.
With the matching adapter database/configuration, import and verify content:

```sh
adapter-api import-creation-metadata creation.ndjson
adapter-api backfill-provider-content
```

The NDJSON header is version 1 plus target generation, followed by typed immutable
records. Import verifies kind, owner/operation or migration entity and digest
against current core refs. Duplicate, conflicting, foreign or missing selected
content fails the transaction. Repeated import is idempotent. Any selected record
not in a recovery file must already exist and verify in PostgreSQL. Lines use the
existing 32 MiB metadata payload bound; parsing is sequential and bounded. Import
and export refuse unreconciled operations. File conversion never opens another
service's tables. Resume the API and wallet with original keys only after the
coherent target and public checks pass.

## Current-state rollback and recovery

After stopping/reconciling writes again, export the API's currently selected
content before stopping its internally available provider:

```sh
adapter-api export-creation-metadata current-creation.ndjson
# Stop the provider, then use its current owner tool:
whole-provider export current-core.json
# For native state use export-native instead.
whole-provider downgrade current-core.json current-creation.ndjson rollback-v1.json
```

Downgrade rehydrates V1 from **current** facts and verified content, including
entities created by compact commands after cutover. It preserves their original
recorded 49/50 bytes, receipts, nonces and events. Import rollback-v1.json into a
fresh store using the frozen predecessor, then restore its adapter/provider
binaries with the matching API/wallet data and keys. An old pre-cutover provider
checkpoint alone loses later work and is not a rollback.

Existing 64 MiB predecessor archive/checkpoint limits still apply; conversion
fails explicitly when its target cannot fit. Public input limits are unchanged.
New files are protected and never overwrite existing files. A failed multi-file
export/conversion is an incomplete cohort; use fresh output paths and keep the
source backup. Full PostgreSQL backups are still needed for sessions, pending
requests, profiles, evidence, conversations and other app data: the metadata
exchange contains only confirmed selected creation content.

A direct successful legacy creation can lack an API record. Its exact preimage
is recoverable from retained call bytes:

```sh
whole-provider recover-creation-metadata current-core.json recovered.ndjson
adapter-api import-creation-metadata recovered.ndjson
```

Recovery excludes rejected calls and verifies the current Creation ref and
creation receipt/entity. Normal API requests never depend on this path. Compact
calls contain hashes only, so their preimages require the API's backup/export.

The real-Sandbox success/restart test can emit disposable cases for replay on the
actual frozen predecessor (known test identities, no deployment data):

```sh
R2A_ROLLBACK_OUTPUT_DIR=/tmp/r2a-case cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked --test core_metadata
# Run the R2a revision of check-core-rollback.py with 84ab1e4 binaries.
# The current script instead verifies the R2b1 V3→V2 cohort.
```

That rehearsal verifies Rust/Shadow/Sandbox predecessor imports after successful
compact worker/Project creation, exact replay, restart and further legacy work.
The existing paging helper remains until R4/R5; this slice introduces no new
storage engine, service or dependency policy exception.
