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
Creation references; neither stores these descriptive preimages. The
current provider uses CoreSnapshot; this document describes
the unchanged creation-content encoding. No empty substitute strings or hidden public DTO flags
are used. API public reads resolve and verify the selected record after applying
the existing visibility policy. Proposal numeric terms and task IDs/assignees remain core facts.
Raw authorization, money operations, command aliases and private case messages do
not depend on creation text. Optional notification titles degrade on missing
content; affected Project/worker presentation fails explicitly.

The adapter compacts accepted public creation requests before signing. Original
public request bytes remain available for idempotency. The runtime routes compact
commands directly; no migration or descriptive content cache is needed.

## Fixed private encoding

SHA-256 commits literal `abako/creation-metadata/v1`, one NUL byte, then SCALE:

1. Generation: 16 bytes.
2. Provenance: discriminant 0 Creation, operation ID (16 bytes) and author (32 bytes).
3. Content: discriminant 0 WorkerName plus its exact SCALE UTF-8 string; or
   discriminant 1 ProjectText plus exact title and description strings.

The domain versions the encoding. Whitespace, control characters and canonically
equivalent Unicode stay distinct. Independent signed vectors are in
`creation-metadata-vectors.json`. Existing SHA-256 is reused; the data-only codec
adds no hashing/signing dependency. Large encoding/decoding runs on blocking
workers at async boundaries. A Creation reference is 49 bytes.
Ordinary contract creation accepts only a Creation commitment (48 bytes).


## Fresh development and recovery

The owner-approved fresh-start amendment removes migration records and converters.
Contracts start through ordinary constructors. Keep matching API, wallet and
provider backups together; the core cannot reconstruct descriptive preimages.
Existing incompatible stores fail startup and are explicitly replaced for a new
development installation. Signed command indexes and current content hashing
remain unchanged.
