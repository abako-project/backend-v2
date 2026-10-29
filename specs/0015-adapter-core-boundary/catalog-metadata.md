# Catalog and lifecycle metadata

Approved ownership, validation and admission rules under SPEC-0015.

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
it and recreation selects a new origin. Seed documents retain exact
categories; a fresh upsert/request origin supplies category requested. This
preserves categories and role associations without predicting creation from a
stale enqueue snapshot. Approval may reuse its request document; numeric Decide
and Delete commands remain unchanged. Use existing scoped internal request reads
when composition needs their association, not all-history expansion of snapshots.

Reason core records retain project/milestone, actor, captured timestamp, effect
kind and ordering, replacing text with a fixed reference. Existing public reason write
routes stay compatible; no new history endpoint is needed.

The existing operation queue gets one catalog-specific FIFO across wallets;
its blockers and wallet FIFO both use strictly earlier creation_sequence. This
ends at each transport operation's known outcome, not at a request remaining
Pending. Ordinary paths do not hydrate catalog content, while the usual wallet
FIFO can still make a later operation wait. API naming rejection uses existing
202/polling with terminal Rejected/error_code and no invented provider outcome.

Defaults are seeded once when the provider is created. Reopening preserves catalog
edits and deletions. API naming guarantees do not cover arbitrary direct core
writes. Raw authorized Delete and DecideReject must permit sequential repair
without whole-graph composition; never release another ID’s same-digest claim.

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


## Fresh development and recovery

The owner-approved fresh-start amendment removes migration records and converters.
Contracts start through ordinary constructors. Keep matching API, wallet and
provider backups together; the core cannot reconstruct descriptive preimages.
Existing incompatible stores fail startup and are explicitly replaced for a new
development installation. Signed command indexes and current content hashing
remain unchanged.
