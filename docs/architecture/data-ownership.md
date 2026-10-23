# Data Ownership

| Owner | Authoritative data | Storage |
|---|---|---|
| mock-provider | Catalog IDs/references and moderated request lifecycle, worker qualifications/calendars, Project core facts, numeric proposal terms, milestones/tasks, reservations, reputation, simulated Bramp holds and funds, nonces, receipts and events | Memory or its own SQLite database |
| wallet | Principal/wallet binding, encrypted seeds, signing jobs, lifecycle and audit | Custody SQLite |
| adapter-api | Credentials, sessions, descriptive profiles, immutable worker directory names, Project root/proposal documents, catalog/request narrative and normalized skill-name admission, lifecycle reason content, queued operations, event ingestion cursors, recipient notifications and read state | Adapter PostgreSQL |
| Leptos | Browser presentation state | No authoritative business database |

Services access each other's HTTP interfaces, not databases. Identifiers link records across boundaries without cross-service foreign keys. Browser DTOs are defined in generated-contracts; service storage models remain private.

Inside the provider, the Rust engine owns business state; the shared
`provider-execution` shell owns generation, nonces, receipts and event cursors.
The SDK stays outside the root workspace. The private whole-provider candidate
can explicitly select Rust, Shadow or Sandbox; the normal mock remains native.
Component fixtures do not enable an application authority switch. One captured time and
entropy input is retained for each new accepted execution, including recorded
domain rejections. Old receipts remain readable without those inputs.

The mock SQLite implementation stores one serialized state row and commits business changes, nonces, receipts and events together. This is an explicit POC size/throughput trade-off. The memory implementation serializes execution too.

Adapter notifications are a recipient-specific projection. Event ingestion and its cursor commit together. Reading notifications does not mark them read. Receipt reconciliation remains authoritative for ambiguous operation delivery.

SQLite and PostgreSQL state survive restarts; mock memory state does not. To
reset the local stack, use a coherent new secret/data directory and PostgreSQL
volume or Compose project name as described in [deployment](../../infra/README.md).
No development SQLite-to-PostgreSQL backfill is supplied. Retention,
personal-data deletion and production backup schedules are not implemented policies.

SPEC-0015 R2a/R2b1/R2b2 makes the API authoritative for immutable worker directory
names, Project root and proposal text, milestone titles, change explanations,
catalog labels/categories, skill-request narrative and lifecycle reasons. Compact
commands and exact preimages commit together in PostgreSQL. Core V4 snapshots and
archives retain context-bound references, numeric terms, captured compensation,
revision and moderated request lifecycle. Current catalog label and actual skill
origin are separate; renames preserve origin/category and role associations.
The existing API operation queue serializes catalog admission, reserves normalized
skill names before signing and publishes actual assigned IDs only with a known
provider outcome. Uncertain submissions retain holds. These are explicit API
rules and immutable owner documents, not caches of a second core engine.

Public views verify selected content after visibility checks and use allocated
core amounts. Authorization and financial operations use raw core facts. R1 child
copies contain tasks only pending R3. Historical rows and review snapshots remain;
there is no text fallback. `adapter-api backfill-provider-content` verifies selected
worker/Project views through provider HTTP without signing or submitting work.
The paging helper remains until R4/R5.

Migration and rollback require matching current core state and API-owned content.
The [R2b2 operator procedure](../../specs/0015-adapter-core-boundary/r2b-catalog-reasons.md)
documents the explicit V3↔V4 exchange, frozen paired predecessor, reconciliation,
protected files and recovery limits. R2a/R2b1 refs survive unchanged. Earlier
transitions remain documented in their historical notes. See
[milestone evidence](../../progress/handoffs/SPEC-0015.md).
