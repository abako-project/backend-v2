# SPEC-0006: Profiles, catalog and adapter persistence

Status: APPROVED — product owner approved implementation on 2026-09-23.

## Profiles

The adapter owns editable descriptive profiles in PostgreSQL. A client profile
has name, company, department, website, description, location and languages.
A worker profile has name, GitHub username, portfolio URL, biography, background,
proficiency, location and languages. Username is the authentication identifier.

APPROVED amendment, 2026-10-08 (BE-A1/BE-A2): worker profiles may also store
an optional, unverified contactEmail, visible only to the owner. It is not used
for authentication. Read-only GET /api/catalog is public so signup can select
real qualifications before account creation. Catalog mutations remain protected.
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

Profile images are the only binary attachment supported by the legacy client
and worker profiles. Each existing profile section may have one replaceable image,
stored by the adapter in PostgreSQL. The owner uploads raw PNG, JPEG or WebP
bytes (at most 1 MiB) with the matching `Content-Type` using
`PUT /api/profiles/me/{section}/image`, where `section` is `client` or `worker`.
The adapter checks the format signature, not only the supplied header. The image
can be read without login at
`GET /api/profiles/{principalId}/{section}/image`; absent profiles/images return
404. Neither private authentication data nor arbitrary files are exposed. The
image URL is independent of the JSON profile projection. Legacy's unrestricted
file size, MIME trust and update authorization are deliberately not ported.

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
routes. The nested write routes remain aliases during frontend transition; no second
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

No `/v1` aliases are added. OpenAPI and the happy-path guide
show both frontends how to use `/api`. Verify profiles' public/private projections,
catalog request authorization and duplicate handling, canonical and nested task
routes, and operation/SSE recovery with PostgreSQL. Run the mock domain suite
against both memory and SQLite. Existing project escrow and SPEC-0005 dispute
freeze are unchanged.

## Project submission brief (BE-B)

APPROVED amendment, 2026-10-08: the adapter stores the descriptive brief needed
by Project Submission. This does not change provider project creation, matching,
planning prices, tasks, escrow or evidence. Title and description stay in the
provider project; they are not duplicated in the brief.

`GET /api/projects/{projectId}/brief` uses the current provider snapshot and the
same participant visibility as the project read. An existing authorized project
without a saved brief returns `200 null`. Missing projects and outsiders return
404. `PUT` is restricted to the project's client and requires the existing session
and CSRF header. Assigned coordinators/workers can read, but cannot write. Neither
route is public. Provider failure returns 503, not a cached authorization decision.

The replacement request is `{expectedRevision, brief}`. The brief contains:

- `summary`: up to 280 Unicode scalar values; empty is permitted.
- `projectType`: Other, SmartContract, Frontend, MVP, Audit or MobileApp.
- `link`: null/omitted or an absolute HTTP(S) URL, at most 2048 UTF-8 bytes,
  without credentials, whitespace or control characters. The adapter never fetches it.
- `objectives` and `constraints`: ordered arrays, each with at most 50 entries.
  Entries are nonblank and at most 2000 Unicode scalar values. Empty arrays are
  permitted; no new business requirement is imposed on legacy drafts.
- `indicativeBudget`: `{currency:"USD", range}`, where range is Below10000,
  From10000To50000, From50000To100000 or Above100000. These preserve the legacy
  dollar ranges. This is a descriptive preference, never converted to operational KVN.
- `delivery`: `{preference}` with WithinOneMonth, OneToThreeMonths or
  ThreeToSixMonths, or `{preference:"SpecificDate", date:"YYYY-MM-DD"}`. The
  date must be a real Gregorian date, year 0001–9999. It is not a contracted window.

Required fields and unknown-field rejection are specified in OpenAPI. The existing
256 KiB request limit still applies. Nullable link is the only omitted brief field.
U+0000 is rejected in descriptive text because PostgreSQL JSONB cannot store it.
Typed JSON mismatch returns 422; semantic validation returns 400, both as ApiError.

First write uses expectedRevision 0 and returns 201 with revision 1. Updates
replace the whole brief and increment its revision once, returning 200. An exact
retry returns 200 with the saved revision only when it is the immediate successor
of the supplied expectedRevision and its body is identical. Other stale writes
return 409 with code `brief_revision_conflict`. Revision inputs are nonnegative
integers below signed BIGINT's maximum; they are independent of proposal and
planning revisions. Preserve exact integers rather than rounding large revisions.

Persist adapter-owned JSONB and revision in a new table, keyed by provider instance
and project ID. A provider reset cannot attach an old brief to a reused ID. Use a
transaction and row locks for concurrent replicas, with a unique key for concurrent
first writes. No provider table is read or written directly, and no cross-service
foreign key is introduced. Startup migration is additive and repeatable.

The frontend must first confirm project creation and retain its receipt's project
ID. Only then save the brief. If saving fails, retry that write with the same project
ID, expectedRevision and body; never repeat project creation. GET allows recovery
after reload. Milestone submission hashes are explicitly outside BE-B.

Verify owner/participant/outsider permissions, CSRF and Origin, malformed inputs,
repeat migration and restart persistence, provider reset isolation, concurrent
writes and exact retries, and unchanged provider state/nonces/balances.


### BE-C3 — Project participant presentation (APPROVED 2026-10-08)

GET `/api/projects/{projectId}/participants` returns client and coordinator account IDs, optional display names, existing public profile sections and image-availability flags. Current project participants only: unauthenticated 401; inaccessible/missing project 404. No usernames, private contact emails, departments, background, image bytes or session secrets. Unmapped provider accounts retain their actual account ID with null name/profile and false image flags. Read-only adapter/PostgreSQL lookup; no migration or provider command changes.

Acceptance: client, coordinator and assigned worker can read; outsider cannot enumerate through this route; private fields stay absent; missing/unavailable provider projects cannot reveal stale participants.
