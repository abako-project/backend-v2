# Data Ownership

| Owner | Authoritative data | Storage |
|---|---|---|
| mock-provider | Catalogues, worker qualifications/calendars, Project core facts, proposals, milestones/tasks, reservations, reputation, simulated Bramp holds and funds, nonces, receipts and events | Memory or its own SQLite database |
| wallet | Principal/wallet binding, encrypted seeds, signing jobs, lifecycle and audit | Custody SQLite |
| adapter-api | Credentials, sessions, descriptive profiles, immutable worker directory names and Project root text, queued operations, event ingestion cursors, recipient notifications and read state | Adapter PostgreSQL |
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

SPEC-0015 R2a makes the API authoritative for immutable worker directory names
and Project root title/description. Queued compact commands and their exact
preimages commit together in PostgreSQL. Core V2 snapshots/archives contain
context-bound references; public views verify and resolve them after visibility
checks. Authorization and financial operations use raw core facts. Proposal/task
content still uses R1 verified child copies until its later ownership cutover;
old copy rows are retained, without a name/text fallback or permanent second
writer. `adapter-api backfill-provider-content` verifies the selected views
through provider HTTP without signing or submitting work.

Migration and rollback require the matching core state **and** API-owned content,
not a provider checkpoint alone. The [R2a operator procedure](../../specs/0015-adapter-core-boundary/r2a-creation.md)
documents the explicit predecessor transition, queue reconciliation, protected
metadata exchange and recovery of successful direct legacy calls.
See [the milestone evidence](../../progress/handoffs/SPEC-0015.md).
