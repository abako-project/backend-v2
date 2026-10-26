# Creation metadata

Approved ownership and encoding rules under SPEC-0015.

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
current provider uses CoreSnapshotV5/CoreArchiveV5; this document describes
the unchanged creation-content encoding. No empty substitute strings or hidden public DTO flags
are used. API public reads resolve and verify the selected record after applying
the existing visibility policy. Proposal numeric terms and task IDs/assignees remain core facts.
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


## Recovery

Use the current [paired owner procedure](../../docs/operations/task-cutover.md) for
task ownership and preserve matching API/core backups. Earlier metadata converter
steps remain in [the frozen instructions](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/specs/0015-adapter-core-boundary/r2a-creation.md).
Never reinterpret a signed format or substitute a stale checkpoint after writes.
