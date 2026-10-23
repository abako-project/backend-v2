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
