# Data Ownership

| Owner | Authoritative data | Storage |
|---|---|---|
| mock-provider | Catalogues, worker qualifications/calendars, Project core facts, numeric proposal terms, milestones/tasks, reservations, reputation, simulated Bramp holds and funds, nonces, receipts and events | Memory or its own SQLite database |
| wallet | Principal/wallet binding, encrypted seeds, signing jobs, lifecycle and audit | Custody SQLite |
| adapter-api | Credentials, sessions, descriptive profiles, immutable worker directory names, Project root text and proposal documents, queued operations, event ingestion cursors, recipient notifications and read state | Adapter PostgreSQL |
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

SPEC-0015 R2a/R2b1 makes the API authoritative for immutable worker directory
names, Project root title/description, proposal titles/descriptions, milestone
titles and change-request explanations. Compact commands and exact preimages
commit together in PostgreSQL. Core V3 snapshots/archives contain context-bound
references plus actual numeric terms, captured compensation policy and revision.
Public views verify selected content after visibility checks and use allocated
core amounts. Authorization and financial operations use raw core facts.

R1 child copies now contain tasks only, pending R3. Historical rows and immutable
review-comment snapshots are retained; there is no text fallback or second writer.
`adapter-api backfill-provider-content` verifies selected views through provider
HTTP without signing or submitting work. Catalog names/moderation and cancellation
or planning-dispute reasons remain R2b2; the paging helper remains until R4/R5.

Migration and rollback require matching current core state **and** API-owned
content. The [R2b1 operator procedure](../../specs/0015-adapter-core-boundary/r2b-proposals.md)
documents the explicit V2↔V3 transition, queue reconciliation, protected metadata
exchange and recovery of successful direct legacy calls. R2a creation references
survive that transition unchanged. See [milestone evidence](../../progress/handoffs/SPEC-0015.md).
