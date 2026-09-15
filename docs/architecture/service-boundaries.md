# Service Boundaries

## Bounded Contexts

See accepted ADR-0001. Adapter, custody, and mock provider are backend deployment boundaries. Leptos and the team's external frontend remain independent consumers of the public REST/SSE contract. Business domains are distinct modules and contract instances inside the transactional mock runtime, not separate database-writing HTTP services.

## Ownership Matrix

Adapter owns credentials, sessions, pending transport operations, and notification read state. Custody owns signing keys and jobs. Mock owns catalogs, worker modes and qualifications, calendars and reservations, projects, proposals, task storages, scores, funds, execution receipts, and provider events.

## Boundary Signals

Separate deployment when there is an actual security or operational boundary. A contract instance or a database table alone is not a deployment boundary.

## Forbidden Coupling

No cross-service database writes, browser access to custody, frontend dependency on backend implementation crates, or business truth in adapter caches. Preparing unsigned bytes never mutates mock business state.

## Synchronous Dependencies

Adapter calls custody and mock through internal authenticated HTTP. All state-changing simulated contract effects execute in one provider transaction.

## Asynchronous Dependencies

Adapter-owned tasks reconcile signing/submission and ingest durable provider events. Persist ingestion cursor with derived notifications; SSE is delivery, not business authority or an implicit read acknowledgement.

## Extraction and Merge Criteria

Extract a runtime domain only after preserving its transactional guarantees through an approved design. A future blockchain provider replaces construction, submission, and reconciliation behind the stable browser API.
