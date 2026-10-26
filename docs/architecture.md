# Architecture

The adapter, wallet and provider are separate processes with separate stores.
Frontends use the public REST/JSON and SSE API. Contract instances are objects
inside one provider runtime, not independently deployed backend services.

| Owner | Authoritative data | Storage |
|---|---|---|
| Adapter | Identity/sessions, profiles, immutable content, catalog names/uniqueness, task text/progress, accepted dispute snapshots, operations and notifications | PostgreSQL |
| Wallet | Principal/wallet binding, encrypted seeds, signing jobs and audit | SQLite |
| Provider | Catalog IDs/qualification facts, calendars, commercial terms, task IDs/allocator/assignees, lifecycle, exact funds, reputation, nonces, receipts and core events | Native memory/SQLite, or embedded runtime checkpoint in its own SQLite |
| Frontend | Presentation and local UI state | No authoritative business store |

Services use each other's authenticated HTTP interfaces, never another service's
database rows. Public DTOs live in `crates/generated-contracts`; storage models
remain private. There are no cross-service database foreign keys. The accepted
[architecture decision](architecture/adr/0001-rest-and-independent-frontends.md)
records the REST/frontend boundary.

## Execution

The normal mock executes native Rust rules. The isolated
[contract provider](../tools/ink-sandbox/README.md) runs Directory, Calendar and
Project WASM in one runtime, with explicit Rust, Shadow and Sandbox modes.
Both use `provider-execution` for generation, signatures, nonces, idempotency,
receipts and event cursors. Wallet owns keys; provider never signs.

For a core mutation, the adapter records the original request and immutable
content, obtains its signature, submits the exact call and reconciles its receipt.
Work is ordered per wallet. An uncertain outcome retains its original intent and
blocks conflicting work. Retrying cannot invent a replacement operation.
Provider state, receipts and events commit atomically; captured time and entropy
are retained for exact replay. Generation prevents retargeting old signed calls.

Native SQLite stores one serialized state row, an explicit development throughput
trade-off. The embedded provider checkpoints complete runtime and child-trie state.
Contracts use ordinary ink fields and keyed collections; Project history scans
still have finite gas/memory costs. No node or block production runs locally.

## Content and tasks

The adapter owns names, descriptions, proposal documents, catalog/request text,
lifecycle reasons and evidence. Core retains contextual commitments and independently
enforces numeric terms, captured compensation, revisions, permissions and lifecycle.
Public reads check visibility, then verify the selected content. Missing/tampered
content fails explicitly; there is no predecessor text fallback or invented data.
Raw core authorization and financial operations use core facts.

Catalog name admission uses the existing operation queue. Skill uniqueness follows
Rust whitespace-collapse/lowercase and full normalized-byte comparison. Pending
uncertain submissions retain reservations. Actual IDs and skill origin are confirmed
in core; the API does not predict them from a stale snapshot. Details and signed
encodings are in the [ownership requirements](../specs/0015-adapter-core-boundary/spec.md).

Task creation/deletion and membership changes are core transactions, including
empty tasks. Core owns the actual global allocator; the API keeps an integrity
mirror. Core confirmation and durable API materialization are both required before
completion. Unchanged-assignee content edits and progress edits commit in the API
with their task result and notifications, without provider signatures/receipts/cursors.
A finalized operation contains exactly one known result source.

Milestone dispute/cancellation admission captures immutable confirmed task content
and an edit hold. Unresolved membership changes block snapshot admission. Uncertainty
retains the hold; a failed dispute releases it; a confirmed freeze rejects later
edits under the lifecycle guard. Planning disputes preserve task editing. Frozen
reads require verified case context and never reconstruct an invented historical
snapshot from current tasks.

## Notifications and visibility

Core Project events select recipients using membership at core execution, including
direct core calls. Task-only visibility is distinct from contractual participant,
private-channel and evaluation access. API content and ingestion timestamps cannot
change a core event's audience.

Provider event ingestion and its recipient notifications/cursor commit together.
API task notifications share the same notification-ID sequencing lock and commit
with their task changes. Notification IDs, not provider/task cursors, drive public
SSE resume and deduplication. Receipt reconciliation remains authoritative when an
operation response is lost. Reading or receiving a notification does not mark it read.

SSE uses the authenticated session, rechecks expiry/revocation, and sends keepalives
every 15 seconds. Delivery reads pages of 100 through a 32-item channel and closes
a stream after a five-second blocked send. Nginx disables buffering/cache. These
are transport bounds, not a measured production connection-capacity guarantee.
There is no broker, webhook consumer or separate ingestion service.

## Recovery

Backups preserve matching API, wallet and provider state plus the original
secrets. Provider state alone omits API-owned content. Stop admission and reconcile
uncertain operations before backup or replacement. The development installation
uses clean data; no historical import or rollback converter is shipped. Existing
incompatible stores fail startup rather than being silently reset. See
[disaster recovery](operations/disaster-recovery.md).
