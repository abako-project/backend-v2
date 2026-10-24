# R2b2: catalog and lifecycle content

Implementation slice of approved SPEC-0015 R2, following accepted R2b1. Move
catalog labels/categories, moderated request content and cancellation/planning
reason text to explicit API contexts. Keep tasks for R3 and storage replacement
for R4/R5. Deliver one working V4 native/WASM/API cohort and current-state V3
rollback; do not stop at the decoder bridge. No live migration/release is implied.

## Ownership and preserved rules

Core retains catalog IDs, fixed coordinator role 1, worker/qualification facts,
skill→role associations, request ID/requester/role IDs/status/assigned skill ID,
admin-only decisions and fixed content references. Those request markers preserve
RNG allocation, one-time decisions and existing signing receipts/events. Request
narrative and name validation belong to the API, without a second request engine.
Pending requests do not block role deletion; committed skill-role references do.
Approval rechecks current roles and allocates max(current skill IDs)+1, preserving
deletion/reuse and exhaustion. Historical requests may reference deleted IDs.

Preserve exact upsert labels; request presentation and approved names use trim().
Request names retain their existing 100-byte trimmed UTF-8 bound; admin labels
retain long text. Skill uniqueness alone uses Rust Unicode split_whitespace,
join(" "), then lowercase. Full normalized bytes (including accepted NUL) are
stored safely, with the existing SHA256 fixed digest index and full-byte
comparison on occupied keys. Roles are not name-unique. Two pending requests may
share a name. No tighter public bound or new crypto dependency.

A catalog entry's current label reference is distinct from its skill origin
reference. Actual creation selects origin, rename preserves it, deletion removes
it and recreation selects a new origin. Seed/migration documents retain exact
categories; a fresh upsert/request origin supplies category requested. This
preserves categories and role associations without predicting creation from a
stale enqueue snapshot. Approval may reuse its request document; numeric Decide
and Delete commands remain unchanged. Use existing scoped internal request reads
when composition needs their association, not all-history expansion of snapshots.

Reason core records retain project/milestone, actor, captured timestamp, effect
kind and ordering, replacing text with a fixed reference. Migration identifies
the source archive and original record position. Existing public reason write
routes stay compatible; no new history endpoint is needed.

The existing operation queue gets one catalog-specific FIFO across wallets;
its blockers and wallet FIFO both use strictly earlier creation_sequence. This
ends at each transport operation's known outcome, not at a request remaining
Pending. Ordinary paths do not hydrate catalog content, while the usual wallet
FIFO can still make a later operation wait. API naming rejection uses existing
202/polling with terminal Rejected/error_code and no invented provider outcome.

Ordinary native SQLite reopen still reseeds missing defaults; imported stores
still skip that step. If reseeding/direct core writes produce a duplicate label
graph, fail closed rather than silently pick a winner or alter data. Raw authorized
Delete and DecideReject, including their claim finalization, must permit sequential
repair without whole-graph composition. Never release another ID's same-digest
claim. Require a content-valid graph before rollback/export to the stricter V3
predecessor; API naming guarantees do not cover arbitrary direct core writes.

## Queue and metadata

Reuse existing enqueue/process/finalize transactions. Store immutable catalog and
reason documents with the queued compact command, keeping original public request
bytes for retries and fixing generation before signing. Confirmed public names
resolve selected core refs; staged writes are not public state. Seed provisioning
is permitted only against matching generation-bound refs from the explicit
reviewed default dataset. Unseeded mode and fixed role 1 remain valid.

Catalog-specific sequencing and durable normalized-name claims prevent competing
approvals/upserts minting IDs before uniqueness is known. Retain the old committed
name while rename/delete is pending. Reserve new names before signing. Pending
conflicts wait within existing queue/expiry bounds; uncertain submission, crash,
lease expiry or ambiguous Expired state never releases a claim. Publish/release
claims with authoritative outcomes in the existing transaction, using actual
assigned IDs. Missing metadata affects catalog presentation/writes, not unrelated
money/lifecycle commands. API-side name validation uses the supported asynchronous
Rejected/errorCode path, without fabricating provider receipts, nonces or events.

## Compatibility and migration

Append compact UpsertCatalogEntryCore, CreateSkillRequestCore, CancelProjectCore
and DisputePlanningCore at indices 54–57. Preserve every previous index/byte.
Freeze a reviewed V3 predecessor which replays retained 54–57 but rejects fresh
execution after replay admission without consuming state/nonce. Do not retain a
second business engine. Current ownership becomes snapshot/archive/engine V4.
One explicit V3↔V4 owner exchange moves only this slice, preserving R2a/R2b1 refs,
all other business state, actual funds/IDs/counters and complete accepted history.

Imported old receipts and exact replay remain byte-identical. Fresh direct legacy
calls cannot retain old skill_name_exists results after names leave core; that
approved guarantee now belongs to the API. Preserve old corpora for migration and
replay, and move naming behavioral regressions to API tests. Do not retain an
onchain name cache/interpreter or forge rejection history to mimic the old trace.
Legacy text lowering runs after replay admission and retains original signatures.

Reconcile queued/ambiguous work before conversion; retain exact source bytes and
coherent PostgreSQL/provider/wallet backups and keys. Import fresh sealed core
stores and verified metadata through their owner tools. Rollback rehydrates the
current V3 state, including successful new compact writes. Missing/corrupt/foreign
or conflicting content fails explicitly. Keep selected and historical records;
protected file exports may be partial on failure and must not overwrite files.

## Acceptance

Verify normalization, long labels/NUL, duplicate pending requests, competing
approvals/upserts, rename/delete/recreate, role guards and request privacy. Cover
both enqueue rollback directions, lost responses, rejected/expired/uncertain
submissions, restart and claim reconciliation. Preserve core permission/money,
capacity, reputation, IDs and event behavior. Test missing content isolation,
metadata exchange, actual compact successes and rollback on frozen V3 API/provider
binaries followed by ordinary REST work in all three authority modes.

Run full app/SDK and scoped dependency gates, rebuild/reproduce changed v5 source
artifacts, and record exact source/binary hashes and limitations. Astra xhigh
reviews the exact complete milestone and scope. Keep public REST/JSON/SSE unchanged;
container publication, deployment, promotion and stable release observation remain
separate owner gates. No service, dependency, generic document registry, storage
engine or runtime-limit increase is added here.

## V4 owner procedure

Current private formats are CoreSnapshotV4/schemaVersion4 and
CoreArchiveV4/version4/engine_version4. The immediate frozen predecessor is
`d215b65c9c58c7353630f203eb9ee1587bf23a52`: paired adapter, wallet, native and
whole-provider executables, with the accepted V3 contract artifacts. Earlier
installations must complete the historical R2a/R2b1 exchanges first. The current
binary does not execute an earlier business engine.

Pause public writes and operation workers, reconcile every pending/ambiguous
operation, and run `adapter-api check-provider-cutover` using the matching API
PostgreSQL. Stop all other writers. Preserve coherent PostgreSQL, provider and
wallet SQLite, original keys/configuration and frozen executables. Export the
stopped predecessor's current owner archive and keep its exact bytes:

```sh
old-whole-provider export-native predecessor-v3.json
# Use export for a predecessor whole-provider store.
whole-provider upgrade predecessor-v3.json core-v4.json catalog.ndjson reasons.ndjson
mock-provider import core-v4.json fresh-native.sqlite
# Or configure a fresh WHOLE_DATABASE_PATH and chosen authority:
whole-provider import core-v4.json
```

Conversion verifies the original generation/root/history, transfers only this
content slice and emits explicit documents bound to the exact source archive
hash. Import the fresh target internally with original root and keys, keeping
public writes paused. Using the paired API data:

```sh
adapter-api import-catalog-metadata catalog.ndjson
adapter-api import-reason-metadata reasons.ndjson
adapter-api backfill-provider-content
```

NDJSON has a version1/generation header. A shared request/approved label/origin
is one record, checked against every selected context. Imports verify exact
digest, generation, namespace, entity, author/source and request roles, plus a
unique current skill-name graph. Partial recovery files omit only already valid
selected records. Missing/foreign/duplicate/conflicting data rolls back SQL import.
Files are create-new with Unix0600; an interrupted multi-file export can leave
partial files, so retain sources and retry to fresh paths. Sequential line bounds
remain32MiB; the offline converter retains64MiB total archive/metadata bounds.
The private authenticated `/internal/lifecycle-reasons?after=0&limit=256` returns
only compact facts with generation/cursor/total, allowing ordered owner exchange
without a public reason-history API or all-history snapshot expansion.

Verify the current public views and coherent target before resuming. Native
ordinary reopen retains its existing reseeding behavior; imported stores remain
sealed without reseeding. Repair invalid names through authorized raw Delete or
Reject, one operation at a time, before name-sensitive work or export. Arbitrary
direct core writes bypass API uniqueness; duplicate graphs fail closed. Missing
catalog text does not force ordinary money or lifecycle paths to hydrate it.

For current-state rollback, pause and reconcile again. While the current provider
is internally available, export selected contexts, then stop it:

```sh
adapter-api export-catalog-metadata current-catalog.ndjson
adapter-api export-reason-metadata current-reasons.ndjson
whole-provider export current-v4.json
# Use export-native for native SQLite.
whole-provider downgrade current-v4.json current-catalog.ndjson current-reasons.ndjson current-v3.json
```

This includes successful new compact writes, actual assigned IDs, categories and
historical request/reason content. Creation/proposal refs, exact funds, counters,
nonces, signed bytes, inputs, receipts and events remain intact. Import into a new
store with frozen d215b65, restore its paired adapter/provider with the same API
PostgreSQL/wallet/keys, and verify reads, exact replay across restart and further
ordinary REST work. A pre-cutover checkpoint would discard later work. Owner
backup still covers sessions, profiles, tasks, comments, evidence, pending/unused
and historical documents beyond this selected exchange.

Successful selected legacy writes or reviewed defaults can recover their exact
preimages through owner tools; compact writes and imported records still require
API backup:

```sh
whole-provider recover-catalog-metadata current-v4.json recovered-catalog.ndjson
whole-provider recover-reason-metadata current-v4.json recovered-reasons.ndjson
adapter-api import-catalog-metadata recovered-catalog.ndjson
adapter-api import-reason-metadata recovered-reasons.ndjson
```

Existing creation/proposal recovery commands remain available. Recovery never
fabricates signatures or fills a missing compact preimage. The disposable proof
case and paired rehearsal use known test identities and isolated stores:

```sh
R2B2_ROLLBACK_OUTPUT_DIR=/tmp/r2b2-case cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked --test catalog_metadata -- --nocapture
python3 scripts/check-core-rollback.py --case /tmp/r2b2-case --candidate-binaries /path/to/current-binaries --predecessor-binaries /path/to/d215b65-binaries
```

These commands describe the intended verification; final observed results belong
in the handoff/PR. They do not authorize a live cutover, release or deployment.

## Observed predecessor limit

The unchanged V4 execution regression accepts a200008-byte planning reason and a
24008-byte catalog label in all three current authorities. Its complete V3
rehydration imports/exports exactly with the frozen native predecessor. The frozen
V3 whole-provider rejects that untouched case with `InvalidState` and leaves no
committed candidate state. This does not establish an exact limit or allocator
cause. Shortened diagnostic copies are not rollback evidence. A separate genuinely
executed small-reason case retains the long label for the paired three-authority
rehearsal; inputs are fixed before hashing/signing, never edited after execution.

Successful target import, reopen and public checks are mandatory compatibility
gates before routing or resuming writes. On failure preserve the complete current
V4 source, API data and exports; retry only into a fresh destination. Report the
actual import error. There is no source truncation, invented public length cutoff,
limit increase, generic error remapping or automatic authority fallback. New
compact content can exceed the frozen WASM cohort's representational capacity, so
whole-provider rollback is conditional on verified target compatibility.
