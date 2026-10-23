# R2a: immutable creation content

This is implementation detail for the approved SPEC-0015 worker-name and Project
title/description slice. It adds no public operation or changed business rule.

## Preparation currently implemented

The adapter captures the exact accepted creation content inside the existing
enqueue transaction, after authorization and queue admission. The content insert,
original request bytes and queued command either all commit or all roll back.
Existing operation IDs identify retries; no new random ID is consumed. The
provider generation is bound before signing, as with evidence commitments.

`worker_directory_metadata` and `project_creation_metadata` are separate context
tables. They contain generation, a 32-byte content digest and serialized JSON
text. JSON escaping preserves accepted NUL and Unicode bytes in PostgreSQL. An
occupied digest must contain the same exact serialized record or the write fails
explicitly. These records do not contain balances, qualifications, calendars,
proposal children or mutable profile/identity names.

**This preparation still queues the original creation commands.** Current reads
and business authority remain unchanged. Compact commands 49/50 must not be sent
until the complete native/WASM handlers, composition, migration and rollback are
verified together. Capturing a record is not a confirmed public creation.

## Fixed private encoding

SHA-256 input is the literal byte domain `abako/creation-metadata/v1` followed by
one NUL byte, then SCALE encoding of `CreationMetadataRecordV1`:

1. Generation: 16 bytes.
2. Provenance: fixed discriminant 0 for Creation, then operation ID (16 bytes)
   and verified author (32 bytes); or discriminant 1 for Migration, then source
   archive hash (32 bytes) and entity. Entity discriminant 0 is a worker account
   (32 bytes), 1 is a Project ID (16 bytes).
3. Content: fixed discriminant 0 for WorkerName and its exact SCALE UTF-8 string;
   or discriminant 1 for ProjectText and its exact title and description strings.

The domain versions the encoding; there is no caller-selected version byte.
Whitespace, control characters and canonically equivalent Unicode sequences
remain distinct. Independently encoded Creation and Migration vectors for both
content kinds, including Unicode/NUL, are in `creation-metadata-vectors.json`.
The data-only codec has no hashing/signing dependency; consumers use existing
SHA-256. Encoding and serialization run on the blocking pool at the async write
boundary.

## Required before the authority cutover

New ordinary commands carry only `MetadataCommitmentV1`, whose operation ID must
equal the signed envelope. A core handler can only introduce a Creation reference.
Migration references may only enter via a fresh, sealed import; they cannot be
passed as ordinary creation inputs. A migration record makes no claim about the
original author or a missing historical creation receipt.

For a migration, source_archive_hash is SHA-256 of the **exact immutable export
file bytes**, including its original whitespace. Save and reuse that file for
reruns, rather than hashing an in-memory JSON value or reserializing it. The
import verifies each record against the retained entity and selected core ref.
Neither this preparation nor a record marked Migration proves that a live export
or data migration has happened.

For signed predecessor commands, lower only after verified-call replay admission;
retain original signed/RecordedCall bytes. Derive the same Creation record from
generation, verified author, operation ID and original accepted content, then
call the single compact core handler. No provider metadata map is needed.
An owner-export recovery path must reconstruct missing records from successful
retained legacy creation calls, verify the confirmed entity/reference and import
idempotently. Exclude rejected calls. Direct calls missing API content must fail
explicitly in affected presentation without blocking unrelated core writes.

Core reads will use explicit WorkerCore/ProjectCore/Snapshot types. Adapter public
composition must check visibility first and verify content kind, generation,
author/operation or migration entity and digest. Do not fabricate empty public
strings, overlay stale proposal/task children, or hydrate every raw authorization
read. Real successful compact creation, migration/restart and current-state
rollback into the pinned predecessor remain acceptance gates.
