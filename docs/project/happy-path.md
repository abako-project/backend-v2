# Marketplace happy path

This guide describes the implemented mock-backed flow. Both the external frontend
and Leptos call the same REST/JSON API. The adapter authenticates users, queues
operations and asks custody to sign; the mock provider executes the signed
business command and owns the resulting state. No browser calls custody or the
provider directly. The [OpenAPI contract](../../contracts/openapi.json) is the
field-level source of truth and is also served at `GET /api/openapi.json`.

## Conceptual flow

1. A client, potential coordinators and workers register. Workers publish their
   qualifications and weekly capacity. An administrator promotes eligible
   coordinators. In a normal mock deployment the client requests a Bramp deposit
   and an operator confirms it; the fixture-only admin funding route is disabled.
2. The client requests a project. The provider assigns a coordinator in
   Coordinator mode. The coordinator quotes a fixed planning fee and duration;
   the client accepts the quote. The coordinator then prepares a proposal.
3. The proposal defines milestones, each with required skills, committed minutes
   and a budget for every worker slot. Creating its draft also creates one task
   storage per milestone. The coordinator must create at least one task in each
   storage before submission. Task edits do not change the contractual budget
   or assignment requirements.
4. The coordinator submits the proposal and delivers the plan. The client accepts
   that delivery, paying the planning fee, then approves execution. Approval
   reserves the full execution budget and worker/coordinator calendar minutes
   atomically. Every required skill must match; role IDs are recorded but are
   not an assignment filter. A worker in Coordinator mode cannot fill a worker
   slot, and vice versa. Eligible workers from earlier milestones are preferred;
   relevant score ranks candidates within that group. Only the first milestone
   enters `InProgress`; the others remain `NotStarted`.
5. Workers report task progress. For each milestone, the coordinator submits a
   versioned deliverable reference and rates each assigned worker. The client
   accepts that exact submission, rates the coordinator and either rates the
   team or delegates its rating to the coordinator. Acceptance releases that
   milestone's escrow and updates minute-weighted reputation. Acceptance starts
   the next milestone, or marks the project `Completed` after the last one.
6. The client and coordinator can read balances, project state and notifications.
   Authenticated SSE streams notifications; replay does not mark them read.

The real-service E2E also repeats steps 3–5 with four milestones and teams of
5/3/4/2 workers. Each milestone has a distinct task storage. This is the
approved redesign, not full wire or behavioral parity with the legacy API.

If the client rejects a delivery, the milestone becomes `ChangesRequested`;
rejection alone does not create a dispute. The client or assigned coordinator
may then open a public dispute against the *current* rejected submission.
Opening freezes the entire project, including all task storages and unpaid
escrow. The other party may publish one response. The case remains Open:
resolution, fund disposition, chat and timeout are not implemented. This
alternative path is tested separately from the successful settlement path.

## Technical flow

Use the same API origin for either frontend, for example
`http://localhost:8088` in the local Compose setup. The adapter uses PostgreSQL
for credentials, sessions, descriptive profiles, transport operations and
notifications; the mock still owns all business state in SQLite or memory.
JSON names are camelCase,
enum values PascalCase, money exact decimal strings, and times Unix seconds.
Calendar windows use `{"isoYear":2026,"week":40}`; choose a future valid ISO
week rather than copying that example. IDs are returned by the provider; do not
invent milestone, storage, submission or dispute IDs.

### Session and operation rules

- `POST /api/auth/register` takes `username`, `password` and `displayName`;
  `POST /api/auth/login` takes `username` and `password`. Both return a
  `SessionView` and set an HttpOnly session cookie. Use
  `GET /api/auth/session` to recover `csrfToken` after a reload.
- Every authenticated mutation sends the cookie, `X-CSRF-Token`, JSON when
  there is a body. Provider business commands also send a new `Idempotency-Key`
  (a 16-byte `0x` hex operation ID). Retry the *same action* with the same key;
  never reuse it for a different body.
- After successful project creation, save its descriptive brief using
  `PUT /api/projects/{projectId}/brief` with `{expectedRevision:0, brief}`.
  This adapter-owned write returns 201/200 directly and needs cookie/CSRF,
  not an operation poll. Recover it through GET (200 null before the first save).
  If saving fails, retain the project ID and retry only the brief write; never
  create the project again. Updates use the brief revision, independent of
  planning/proposal revisions. An exact immediate retry returns the saved result;
  stale/different content returns 409 `brief_revision_conflict`. See OpenAPI for
  legacy project types, USD budget preferences and delivery choices.
- Business mutations return HTTP 202 with `operationId`. Poll
  `GET /api/operations/{operationId}` until `status` is `Finalized`.
  Require `receipt.outcome.type == "Success"`; 202 or Finalized alone does
  not mean business success. A created entity's ID is
  `receipt.createdEntityId`. A failed receipt exposes a domain error code.
  `OutcomeUnknown` needs reconciliation with the same ID, not a new command.
- Read the current project before each acceptance or approval and send its
  `planning.revision` or `proposal.revision` as `expectedRevision`.
  A stale revision fails without paying or reserving anything.

For example, after login:

```javascript
const api = "http://localhost:8088";
const session = await fetch(`${api}/api/auth/session`, {
  credentials: "include",
}).then(r => r.json());

async function command(method, path, body) {
  const key = `0x${crypto.randomUUID().replaceAll("-", "")}`;
  const response = await fetch(api + path, {
    method,
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": session.csrfToken,
      "Idempotency-Key": key,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw await response.json();
  const reference = await response.json(); // HTTP 202
  for (;;) {
    const status = await fetch(`${api}/api/operations/${reference.operationId}`, {
      credentials: "include",
    }).then(r => r.json());
    if (status.status === "Finalized") {
      if (status.receipt.outcome.type !== "Success") throw status.receipt.outcome;
      return status.receipt;
    }
    if (status.status === "Rejected" || status.status === "Expired") throw status;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
}
```

This small example omits a client-side deadline and network-error handling;
production clients must bound polling and retain the key across retries.
The credentialed CORS origin must be in `ALLOWED_ORIGINS`. Session cookies are
`SameSite=Lax`; unrelated cross-site hosting needs a separately approved
cookie/TLS policy.

### Requests in order

The table shows one milestone. Replace brace-delimited route parameters with IDs
from receipts or the latest project read. The administrator is the local seeded
admin, not an ordinary user. All listed business writes use `command` above;
profile updates return their profile directly. The E2E fixture enables
`ENABLE_MOCK_FUNDING=true` to use its admin funding row. Without that flag,
use the three Bramp rows instead.

| Actor | Request | Body / result to use next |
|---|---|---|
| Each person | `POST /api/auth/register` | `{"username":"alice","password":"<12+ bytes>","displayName":"Alice"}`; retain the cookie and `accountId`. |
| Client or worker | `PUT /api/profiles/me` | `{"section":"client","profile":{"name":"Alice","company":null,"department":null,"website":null,"description":null,"location":null,"languages":[]}}` or `section: "worker"` with its worker-profile fields. `GET /api/profiles/me` returns the owner's full profile; `GET /api/profiles/{principalId}` returns only public fields. This adapter-owned edit is not a signed provider operation. |
| Worker and coordinator candidate | `POST /api/workers` | `{"displayName":"Alice","qualifications":{"roleIds":[3],"skillIds":[1,5,13]},"calendar":{"defaultWeeklyMinutes":600,"overrides":[]}}`. The seeded catalog is at `GET /api/catalog`. |
| Administrator | `POST /api/admin/coordinators` | `{"account":"<coordinator accountId>"}`. |
| Promoted coordinator | `PUT /api/workers/me/mode` | `{"mode":"Coordinator"}`. |
| Client | `POST /api/bramp/deposits` | `{"amount":"10000"}`; receipt yields `depositId` in `createdEntityId`. This creates no spendable balance. |
| Client or administrator | `GET /api/bramp/deposits/{depositId}` | Read the pending or confirmed request; unrelated accounts cannot read it. |
| Administrator | `POST /api/admin/bramp/deposits/{depositId}/confirm` | No body. This credits the fixed amount once. No bank or currency conversion is involved. |
| Administrator, fixture only | `POST /api/admin/fund` | `{"account":"<client accountId>","amount":"10000"}`. Requires `ENABLE_MOCK_FUNDING=true`; disabled in normal Compose. |
| Client | `POST /api/projects` | `{"title":"Signed Project","description":"Integration flow"}`; receipt yields `projectId`. `GET /api/projects/{projectId}` reveals its assigned `coordinator`. |
| Assigned coordinator | `POST /api/projects/{projectId}/planning/quote` | `{"fee":"100","minutes":100,"window":{"start":{"isoYear":2026,"week":40},"end":{"isoYear":2026,"week":40}}}`. |
| Client | `POST /api/projects/{projectId}/planning/accept` | `{"expectedRevision":<current planning.revision>}`. |
| Coordinator | `POST /api/projects/{projectId}/proposals` | `{"title":"Implementation","description":"One milestone","milestones":[{"key":1,"title":"Ship","window":<future week window>,"coordinatorFee":"100","coordinatorMinutes":60,"requirements":[{"key":1,"roleId":2,"skillIds":[1,5,13],"minutes":120,"budget":"900"}]}]}`. Read the project to obtain `proposalId`, `milestoneId` and `taskStorage.taskStorageId`. |
| Coordinator | `POST /api/task-storages/{storageId}/tasks` | `{"title":"Implement","description":"Tracked separately","taskType":"Task","priority":"Medium","status":"ToDo","assignees":[],"estimatedMinutes":120,"loggedMinutes":0,"dueAt":null}`. Create at least one task per milestone before submission. |
| Coordinator | `POST /api/projects/{projectId}/proposals/{proposalId}/submit` | No JSON body. |
| Client | `POST /api/projects/{projectId}/planning/accept-delivery` | `{"expectedRevision":<current planning.revision>}`; planning fee is settled. |
| Client | `POST /api/projects/{projectId}/proposals/{proposalId}/approve` | `{"expectedRevision":<current proposal.revision>}`; inspect assignments and execution escrow in a fresh project read. |
| Coordinator | `PUT /api/task-storages/{storageId}/tasks/{taskId}` | Full `TaskDefinition` with an assigned worker in `assignees`. |
| Assigned worker | `PATCH /api/task-storages/{storageId}/tasks/{taskId}` | `{"status":"Done","loggedMinutes":120}`. Logged time does not alter the reserved contractual minutes. |
| Coordinator | `POST /api/projects/{projectId}/milestones/{milestoneId}/completion-submissions` | `{"workerRatings":[{"worker":"<assigned accountId>","score":8}],"deliverable":{"url":"https://example.test/delivery","sha256":"0x<64 hex characters>"}}`. Receipt yields `submissionId`. The older `request-completion` path is an alias. The hash must describe the actual evidence bytes; the API does not fetch or verify the URL. |
| Client | `POST /api/projects/{projectId}/milestones/{milestoneId}/accept-completion` | `{"submissionId":"<current submissionId>","coordinatorScore":9,"teamRating":{"type":"Client","score":6}}`. Alternatively use `{"type":"DelegateToCoordinator"}` for `teamRating`. This settles that milestone only. |

`GET /api/task-storages/{storageId}` and
`GET /api/task-storages/{storageId}/tasks/{taskId}` read the same provider-owned
state. The older project-nested task write paths remain aliases during frontend
transition; neither path creates a second storage.

Optional auxiliary flows use the same session and signing pipeline. A worker
may `POST /api/catalog/skill-requests` with
`{"name":"New skill","roleIds":[3]}` and read its own requests at
`GET /api/catalog/skill-requests/me`. The operator reviews
`GET /api/admin/catalog/skill-requests` and posts `"Approve"` or `"Reject"`
to `/api/admin/catalog/skill-requests/{requestId}/decision`. Approval adds the
skill to the global catalog; the worker must separately update qualifications.
A funded user may `POST /api/bramp/withdrawals` with `{"amount":"100"}`;
the pending request holds only free KVN and may be cancelled at
`POST /api/bramp/withdrawals/{withdrawalId}/cancel`. No bank transfer occurs.
Passkey registration starts at `POST /api/auth/passkeys/register/options`
with the current password and completes at `/register/verify` with the
WebAuthn credential. Password login remains available. To sign in with a
passkey, send `POST /api/auth/passkeys/login/options` with
`{"username":"alice"}`. Pass the returned `options` to the browser WebAuthn
API and send its JSON credential with the returned `ceremonyId` to
`POST /api/auth/passkeys/login/verify`:
`{"ceremonyId":"…","credential":{…}}`. The response sets the same HttpOnly
session cookie and returns the same `SessionView` and CSRF token as password
login. No verified email is needed. Challenges expire after five minutes and
are single-use; an unknown username and an account without passkeys receive
the same `401 invalid_credentials` error.

After every business write, poll its operation before issuing a dependent write.
The client may use `GET /api/projects/{projectId}`; each participant can read
`GET /api/balance` and `GET /api/workers` for public score summaries.
For the four-milestone case, put four entries in `milestones` with stable keys
1–4 and 5/3/4/2 requirements; create tasks under each returned storage ID.
After approval, expect status sequence `InProgress, NotStarted, NotStarted,
NotStarted`. Submit and accept each milestone using *its own* milestone and
submission IDs; check that the next one starts and the final acceptance sets
`project.completed` to `true`. The executable fixture is
[`scripts/poc-e2e.py`](../../scripts/poc-e2e.py).

### Notifications and rejection branch

`GET /api/notifications?after=0` returns a page. `GET /api/events`
streams authenticated SSE `notification` events; reconnect using the last
`notificationId` as `Last-Event-ID` or `after`. Delivery and replay do
not mark a notification read. Use
`POST /api/notifications/{notificationId}/read` for that.

Instead of acceptance, the client may call
`POST /api/completion-submissions/{submissionId}/rejection` with
`{"evidence":{"url":"https://example.test/reason","sha256":"0x<64 hex characters>"}}`.
Only while that rejection remains current, the client or coordinator may call
`POST /api/disputes` with `projectId`, `milestoneId`,
`rejectedSubmissionId` and the same shape of `evidence`. The created
`disputeId` is in the receipt. Anyone can then call
`GET /api/disputes/{disputeId}` without login. The other party may call
`POST /api/disputes/{disputeId}/response` once with an evidence reference.
No API exists to resolve or unlock the case. See [SPEC-0005](../../specs/0005-dispute-opening/spec.md).

## Run the executable flow

```sh
cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked
python3 scripts/poc-e2e.py
```

The script requires PostgreSQL's `initdb`, `pg_ctl` and `psql` commands. It
starts a disposable PostgreSQL instance and local services, then runs the single-milestone
happy path, the four-milestone 5/3/4/2 path, the dispute branch and auxiliary
profile/Bramp/catalog checks on both SQLite and memory mock storage. It also
scans bounded service logs for the generated secret markers. Use `--storage sqlite` or `--storage memory`
to run only one backend. It is a signed backend E2E, not a browser E2E or a
real-chain test.
