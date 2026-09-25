# Data Ownership

| Owner | Authoritative data | Storage |
|---|---|---|
| mock-provider | Catalogues, workers/calendars, projects, proposals, milestones/tasks, reservations, reputation, simulated Bramp holds and funds, nonces, receipts and events | Memory or its own SQLite database |
| wallet | Principal/wallet binding, encrypted seeds, signing jobs, lifecycle and audit | Custody SQLite |
| adapter-api | Credentials, sessions, descriptive profiles, queued operations, event ingestion cursors, recipient notifications and read state | Adapter PostgreSQL |
| Leptos | Browser presentation state | No authoritative business database |

Services access each other's HTTP interfaces, not databases. Identifiers link records across boundaries without cross-service foreign keys. Browser DTOs are defined in generated-contracts; service storage models remain private.

The mock SQLite implementation stores one serialized state row and commits business changes, nonces, receipts and events together. This is an explicit POC size/throughput trade-off. The memory implementation serializes execution too.

Adapter notifications are a recipient-specific projection. Event ingestion and its cursor commit together. Reading notifications does not mark them read. Receipt reconciliation remains authoritative for ambiguous operation delivery.

SQLite and PostgreSQL state survive restarts; mock memory state does not. To
reset the local stack, use a coherent new secret/data directory and PostgreSQL
volume or Compose project name as described in [deployment](../../infra/README.md).
No development SQLite-to-PostgreSQL backfill is supplied. Retention,
personal-data deletion and production backup schedules are not implemented policies.
