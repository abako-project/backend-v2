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
   coordinators; in the seeded local mock the administrator can fund the client.
2. The client requests a project. The provider assigns a coordinator in
   Coordinator mode. The coordinator quotes a fixed planning fee and duration;
   the client accepts the quote. The coordinator then prepares a proposal.
3. The proposal defines milestones, each with required skills, committed minutes
   and a budget for every worker slot. Creating its draft also creates one task
   storage per milestone. The coordinator can add and edit tasks there without
   changing the contractual budget or assignment requirements.
4. The coordinator submits the proposal and delivers the plan. The client accepts
   that delivery, paying the planning fee, then approves execution. Approval
   reserves the full execution budget and worker/coordinator calendar minutes
   atomically. Every required skill must match; role IDs are recorded but are
   not an assignment filter. A worker in Coordinator mode cannot fill a worker
   slot, and vice versa. All milestones enter `InProgress` together.
5. Workers report task progress. For each milestone, the coordinator submits a
   versioned deliverable reference and rates each assigned worker. The client
   accepts that exact submission, rates the coordinator and either rates the
   team or delegates its rating to the coordinator. Acceptance releases that
   milestone's escrow and updates minute-weighted reputation.
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
`http://localhost:8088` in the local Compose setup. JSON names are camelCase,
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
  there is a body, and a new `Idempotency-Key` (a 16-byte `0x` hex operation
  ID). Retry the *same action* with the same key; never reuse it for a
  different body.
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
admin, not an ordinary user. All listed writes use `command` above.

| Actor | Request | Body / result to use next |
|---|---|---|
| Each person | `POST /api/auth/register` | `{"username":"alice","password":"<12+ bytes>","displayName":"Alice"}`; retain the cookie and `accountId`. |
| Worker and coordinator candidate | `POST /api/workers` | `{"displayName":"Alice","qualifications":{"roleIds":[3],"skillIds":[1,5,13]},"calendar":{"defaultWeeklyMinutes":600,"overrides":[]}}`. The seeded catalog is at `GET /api/catalog`. |
| Administrator | `POST /api/admin/coordinators` | `{"account":"<coordinator accountId>"}`. |
| Promoted coordinator | `PUT /api/workers/me/mode` | `{"mode":"Coordinator"}`. |
| Administrator, mock seed only | `POST /api/admin/fund` | `{"account":"<client accountId>","amount":"10000"}`. This is not a production funding API. |
| Client | `POST /api/projects` | `{"title":"Signed POC","description":"Integration flow"}`; receipt yields `projectId`. `GET /api/projects/{projectId}` reveals its assigned `coordinator`. |
| Assigned coordinator | `POST /api/projects/{projectId}/planning/quote` | `{"fee":"100","minutes":100,"window":{"start":{"isoYear":2026,"week":40},"end":{"isoYear":2026,"week":40}}}`. |
| Client | `POST /api/projects/{projectId}/planning/accept` | `{"expectedRevision":<current planning.revision>}`. |
| Coordinator | `POST /api/projects/{projectId}/proposals` | `{"title":"Implementation","description":"One milestone","milestones":[{"key":1,"title":"Ship","window":<future week window>,"coordinatorFee":"100","coordinatorMinutes":60,"requirements":[{"key":1,"roleId":2,"skillIds":[1,5,13],"minutes":120,"budget":"900"}]}]}`. Read the project to obtain `proposalId`, `milestoneId` and `taskStorage.taskStorageId`. |
| Coordinator | `POST /api/projects/{projectId}/task-storages/{storageId}/tasks` | `{"title":"Implement","description":"Tracked separately","taskType":"Task","priority":"Medium","status":"ToDo","assignees":[],"estimatedMinutes":120,"loggedMinutes":0,"dueAt":null}`. |
| Coordinator | `POST /api/projects/{projectId}/proposals/{proposalId}/submit` | No JSON body. |
| Client | `POST /api/projects/{projectId}/planning/accept-delivery` | `{"expectedRevision":<current planning.revision>}`; planning fee is settled. |
| Client | `POST /api/projects/{projectId}/proposals/{proposalId}/approve` | `{"expectedRevision":<current proposal.revision>}`; inspect assignments and execution escrow in a fresh project read. |
| Coordinator | `PUT /api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}` | Full `TaskDefinition` with an assigned worker in `assignees`. |
| Assigned worker | `PATCH /api/projects/{projectId}/task-storages/{storageId}/tasks/{taskId}/progress` | `{"status":"Done","loggedMinutes":120}`. Logged time does not alter the reserved contractual minutes. |
| Coordinator | `POST /api/projects/{projectId}/milestones/{milestoneId}/completion-submissions` | `{"workerRatings":[{"worker":"<assigned accountId>","score":8}],"deliverable":{"url":"https://example.test/delivery","sha256":"0x<64 hex characters>"}}`. Receipt yields `submissionId`. The older `request-completion` path is an alias. The hash must describe the actual evidence bytes; the API does not fetch or verify the URL. |
| Client | `POST /api/projects/{projectId}/milestones/{milestoneId}/accept-completion` | `{"submissionId":"<current submissionId>","coordinatorScore":9,"teamRating":{"type":"Client","score":6}}`. Alternatively use `{"type":"DelegateToCoordinator"}` for `teamRating`. This settles that milestone only. |

After every write, poll its operation before issuing a dependent write.
The client may use `GET /api/projects/{projectId}`; each participant can read
`GET /api/balance` and `GET /api/workers` for public score summaries.
For the four-milestone case, put four entries in `milestones` with stable keys
1–4 and 5/3/4/2 requirements; create tasks under each returned storage ID.
After approval, submit and accept each milestone using *its own* milestone
and submission IDs. The executable fixture is
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

The script starts disposable local services and runs the single-milestone
happy path, the four-milestone 5/3/4/2 path and the dispute branch on both
SQLite and memory mock storage. Use `--storage sqlite` or `--storage memory`
to run only one backend. It is a signed backend E2E, not a browser E2E or a
real-chain test.
