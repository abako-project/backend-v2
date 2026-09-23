# SPEC-0006: Profiles, catalog and adapter persistence

Status: APPROVED — product owner approved implementation on 2026-09-23.

## Profiles

The adapter owns editable descriptive profiles in PostgreSQL. A client profile
has name, company, department, website, description, location and languages.
A worker profile has name, GitHub username, portfolio URL, biography, background,
proficiency, location and languages. Email remains private authentication data.
Qualifications, mode, calendar, scores and project history remain provider-owned
and are not copied into these rows. Availability is read from the worker's
calendar, not a second profile field.

`GET /api/profiles/me` and `PUT /api/profiles/me` require a session and return
the full profile for its owner. `GET /api/profiles/{principalId}` returns only
name, biography/description, languages, general location, professional links
and company where applicable. It excludes email, department, auth/session data,
custody data and unpublished fields. Only the owner may edit; the adapter derives
the principal ID from the session, never from an update body. Inputs are bounded,
validated and cannot contain arbitrary extra fields. Missing optional fields
have an explicit null/empty representation in the OpenAPI schema.

## Catalog

The provider owns the existing seeded roles/skills and skill-to-role associations.
Associations are descriptive metadata; matching still checks every required
skill and never filters by role. A worker may select existing skills or submit
a new-skill request. Requests do not immediately alter the global catalog or
worker qualifications. Only the system origin can approve a request by adding
the skill and its associated role IDs, or reject it. Duplicate normalized skill
names cannot create duplicate catalog entries. Coordinator role ID 1 remains
fixed and cannot be renamed/deleted. Requests and decisions live in the mock
business transaction and emit durable events.

Routes: `POST /api/catalog/skill-requests`,
`GET /api/catalog/skill-requests/me`, and
`POST /api/admin/catalog/skill-requests/{requestId}/decision`. Existing
catalog/qualification routes stay available. The decision body selects Approve
or Reject; approval uses the existing privileged catalog rules.

## Task-storage routes

`GET /api/task-storages/{storageId}`,
`GET /api/task-storages/{storageId}/tasks/{taskId}`,
`POST /api/task-storages/{storageId}/tasks`, and
`PUT/PATCH /api/task-storages/{storageId}/tasks/{taskId}` are canonical.
The adapter resolves project/milestone ownership from the provider and applies
the same signed authorization and dispute freeze as the existing project-nested
routes. The nested routes remain aliases during frontend transition; no second
task store is introduced. Existing E2E requests remain valid.

## Adapter PostgreSQL

Only adapter-owned data (principals, password hashes, sessions, profiles,
WebAuthn credentials/challenges, transport operations, notifications and read
state) moves to PostgreSQL. Custody and mock keep their current independent
storage. Fresh PostgreSQL migrations create the adapter schema; no SQLite data
backfill or deletion is authorized. The adapter uses `ADAPTER_DATABASE_URL`
when set; otherwise it uses the PostgreSQL service and development credentials
defined in `infra/compose.yaml`. External deployments must explicitly set a
secure connection string. Startup fails visibly if the database or migration is
unavailable. Readiness reports database health.

Claims, leases, idempotency and notification cursor updates retain their
current semantics under concurrent adapter replicas. Use PostgreSQL row locks,
unique constraints and transactions rather than a process-local mutex. Each
service owns only its tables; no direct cross-service database access. The
Compose database has a health check and the adapter waits for readiness. Do not
place deployment credentials or user data in the repository.

## Compatibility and verification

No `/v1` aliases or GraphQL schema are added. OpenAPI and the happy-path guide
show both frontends how to use `/api`. Verify profiles' public/private projections,
catalog request authorization and duplicate handling, canonical and nested task
routes, and operation/SSE recovery with PostgreSQL. Run the mock domain suite
against both memory and SQLite. Existing project escrow and SPEC-0005 dispute
freeze are unchanged.
