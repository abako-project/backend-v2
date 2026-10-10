#!/usr/bin/env python3
"""Exercise real custodial REST operations against disposable local services.

Run after `cargo build --workspace --all-features --locked`. PostgreSQL binaries
(`initdb`, `pg_ctl`, `psql`) are required; no Docker or real tokens are used.
"""

import argparse
from contextlib import ExitStack
from datetime import date, timedelta
import http.client
import http.cookiejar
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
ORIGINS = ("http://localhost:8088", "http://localhost:5173")


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def available_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def temporary_postgres(stack, directory):
    data = directory / "postgres"
    socket_dir = directory / "pg-socket"
    socket_dir.mkdir()
    port = available_port()
    subprocess.run(["initdb", "-D", str(data), "-U", "kunveno", "-A", "trust",
                    "--no-instructions"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["pg_ctl", "-D", str(data), "-o",
                    f"-F -c listen_addresses=127.0.0.1 -p {port} -k {socket_dir}",
                    "-w", "start"], check=True, stdout=subprocess.DEVNULL)
    stack.callback(subprocess.run, ["pg_ctl", "-D", str(data), "-m", "immediate",
                                   "-w", "stop"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["psql", "-h", "127.0.0.1", "-p", str(port), "-U", "kunveno",
                    "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-c",
                    "CREATE DATABASE kunveno"], check=True, stdout=subprocess.DEVNULL)
    return f"postgres://kunveno@127.0.0.1:{port}/kunveno"


class Client:
    secret_markers = ()

    def __init__(self, base, origin=ORIGINS[0]):
        self.base = base
        self.origin = origin
        self.cookies = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}),
            urllib.request.HTTPCookieProcessor(self.cookies),
        )
        self.session = None

    def request(self, method, path, body=None, expected=200, headers=None):
        fields = {"Origin": self.origin}
        if self.session:
            fields["X-CSRF-Token"] = self.session["csrfToken"]
        if body is not None:
            fields["Content-Type"] = "application/json"
        fields.update(headers or {})
        request = urllib.request.Request(
            self.base + path,
            data=None if body is None else json.dumps(body).encode(),
            headers=fields,
            method=method,
        )
        try:
            response = self.opener.open(request, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            for name, marker in self.secret_markers:
                require(marker not in raw, f"{method} {path} disclosed {name}")
            try:
                value = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                value = None
            # Do not include request bodies, cookies, or server error text.
            code = value.get("code", "unknown") if isinstance(value, dict) else "non-json"
            allowed = (expected,) if isinstance(expected, int) else expected
            require(response.status in allowed,
                    f"{method} {path}: expected {expected}, got {response.status} ({code})")
            return value

    def authenticate(self, path, body):
        self.session = self.request("POST", path, body, 201 if path.endswith("/register") else 200)
        return self.session["accountId"]

    def command(self, method, path, body=None, operation_id=None, expected_outcome="Success"):
        key = operation_id or "0x" + secrets.token_hex(16)
        reference = self.request(method, path, body, 202, {"Idempotency-Key": key})
        require(reference["operationId"] == key, "adapter changed caller idempotency key")
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            operation = self.request("GET", "/api/operations/" + key)
            if operation["status"] == "Finalized":
                receipt = operation["receipt"]
                require(receipt["outcome"]["type"] == expected_outcome,
                        f"expected {expected_outcome}, got {receipt['outcome'].get('type')}: "
                        f"{receipt['outcome'].get('code')}")
                return operation
            require(operation["status"] not in ("Rejected", "Expired"),
                    f"operation terminated: {operation.get('errorCode')}")
            time.sleep(0.1)
        raise AssertionError(f"operation did not finalize: {key}")

    def first_sse_after(self, cursor):
        request = urllib.request.Request(
            self.base + "/api/events",
            headers={"Origin": self.origin, "Last-Event-ID": str(cursor)},
        )
        with self.opener.open(request, timeout=10) as response:
            require("text/event-stream" in response.headers.get("Content-Type", ""),
                    "events endpoint did not return SSE")
            event_id = None
            data = []
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                line = response.readline().decode().rstrip("\r\n")
                for name, marker in self.secret_markers:
                    require(marker not in line.encode(), f"SSE disclosed {name}")
                if line.startswith("id:"):
                    event_id = int(line[3:].strip())
                elif line.startswith("data:"):
                    data.append(line[5:].strip())
                elif not line and data:
                    value = json.loads("\n".join(data))
                    require(event_id == value["notificationId"] and event_id > cursor,
                            "SSE did not resume after the authorized cursor")
                    return value
            raise AssertionError("SSE did not emit a persisted notification")


class LossyProvider(ThreadingHTTPServer):
    """Drop one successful HTTP reply, after the real provider has committed."""

    daemon_threads = True

    def __init__(self, target):
        super().__init__(("127.0.0.1", 0), ForwardRequest)
        self.target = target
        self.drop_next = False
        self.dropped = 0

    def handle_error(self, _request, _client_address):
        # Intentional connection loss must not print headers or request bodies.
        pass


class ForwardRequest(BaseHTTPRequestHandler):
    def log_message(self, _format, *_args):
        pass

    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def forward(self):
        size = int(self.headers.get("Content-Length", 0))
        if size > 1024 * 1024:
            self.send_error(413)
            return
        body = self.rfile.read(size) if size else None
        headers = {key: value for key, value in self.headers.items()
                   if key.lower() not in ("host", "connection", "transfer-encoding")}
        connection = http.client.HTTPConnection("127.0.0.1", self.server.target, timeout=6)
        try:
            connection.request(self.command, self.path, body, headers)
            response = connection.getresponse()
            payload = response.read()
            if (self.command == "POST" and self.path == "/internal/contracts/call"
                    and response.status == 200 and self.server.drop_next):
                self.server.drop_next = False
                self.server.dropped += 1
                self.close_connection = True
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            self.send_response(response.status)
            self.send_header("Content-Type", response.getheader("Content-Type", "application/json"))
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        finally:
            connection.close()


def wait_ready(client, process):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        require(process.poll() is None, "service exited before becoming ready")
        try:
            client.request("GET", "/ready", expected=(200, 204))
            return
        except (OSError, AssertionError):
            time.sleep(0.1)
    raise AssertionError("service did not become ready within 20 seconds")


def stop_process(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def launch(stack, binary, port, environment, logs=None):
    log = stack.enter_context(tempfile.TemporaryFile())
    if logs is not None:
        logs.append((binary.name, log))
    process = subprocess.Popen(
        [str(binary)], cwd=ROOT, stdout=log, stderr=log,
        env={**os.environ, **environment, "BIND_ADDR": f"127.0.0.1:{port}"},
    )
    stack.callback(stop_process, process)
    client = Client(f"http://127.0.0.1:{port}")
    wait_ready(client, process)
    return client


def exercise_project_brief(client, coordinator, worker, outsider, project_path):
    """Descriptive adapter writes must not change signed provider state or funds."""
    path = project_path + "/brief"
    project = client.request("GET", project_path)
    balances = [actor.request("GET", "/api/balance") for actor in (client, coordinator, worker)]
    require(client.request("GET", path) is None, "new project already has a brief")
    outsider.request("GET", path, expected=404)
    worker.request("GET", path, expected=404)
    doc = json.loads((ROOT / "contracts/openapi.json").read_text())
    brief = doc["components"]["schemas"]["ProjectBrief"]["examples"][0]
    body = {"expectedRevision": 0, "brief": brief}
    coordinator.request("PUT", path, body, 403)
    outsider.request("PUT", path, body, 404)
    client.request("PUT", path, body, 403, {"X-CSRF-Token": "forged"})
    saved = client.request("PUT", path, body, 201)
    require(saved["revision"] == 1 and saved["brief"] == brief, "brief lost its fields or order")
    require(client.request("PUT", path, body) == saved, "exact initial retry changed revision")
    require(coordinator.request("GET", path) == saved, "coordinator cannot read the project brief")
    body = {"expectedRevision": 1, "brief": {**brief, "summary": "Updated project summary"}}
    updated = client.request("PUT", path, body)
    require(updated["revision"] == 2, "brief update did not increment once")
    require(client.request("PUT", path, body) == updated, "exact update retry changed revision")
    stale = client.request("PUT", path, {**body, "brief": brief}, 409)
    require(stale["code"] == "brief_revision_conflict", "stale edit overwrote the brief")
    require(client.request("GET", project_path) == project, "brief write changed provider project state")
    require([actor.request("GET", "/api/balance") for actor in (client, coordinator, worker)] == balances,
            "descriptive budget changed available funds")


def prepare_single(base, admin_password):
    admin = Client(base)
    admin.authenticate("/api/auth/login", {"username": "admin", "password": admin_password})
    require(admin.session["isAdmin"], "bootstrap administrator missing")
    people = {}
    password = secrets.token_urlsafe(24)
    for name in ("client", "coordinator", "worker", "outsider"):
        actor = Client(base, ORIGINS[len(people) % 2])
        actor.authenticate("/api/auth/register", {
            "username": name + secrets.token_hex(4), "password": password, "displayName": name,
        })
        require(not actor.session["isAdmin"], "registration granted administrative authority")
        people[name] = actor
    client, coordinator, worker, outsider = (people[name] for name in people)
    catalog = client.request("GET", "/api/catalog")
    require(len(catalog["roles"]) == 9 and len(catalog["skills"]) == 33, "legacy catalog seed differs")
    require(any(role["id"] == 1 and role["fixed"] for role in catalog["roles"]), "coordinator role not fixed")
    for actor in (coordinator, worker):
        actor.command("POST", "/api/workers", {
            "displayName": actor.session["displayName"],
            "qualifications": {"roleIds": [3], "skillIds": [1, 5, 13]},
            "calendar": {"defaultWeeklyMinutes": 600, "overrides": []},
        })
    coordinator_account, worker_account = (actor.session["accountId"] for actor in (coordinator, worker))
    admin.command("POST", "/api/admin/coordinators", {"account": coordinator_account})
    coordinator.command("PUT", "/api/workers/me/mode", {"mode": "Coordinator"})
    admin.command("POST", "/api/admin/fund", {"account": client.session["accountId"], "amount": "10000"})
    client.request("POST", "/api/projects", {"title": "Forged", "description": ""}, 403,
                   {"X-CSRF-Token": "forged"})
    outsider.request("POST", "/api/admin/fund", {"account": worker_account, "amount": "1"}, 403)
    client.request("GET", "/api/catalog", expected=403, headers={"Origin": "https://untrusted.invalid"})
    project_op = client.command("POST", "/api/projects", {"title": "Signed POC", "description": "Integration flow"})
    project_id = project_op["receipt"]["createdEntityId"]
    project_path = "/api/projects/" + project_id
    project = client.request("GET", project_path)
    require(project["coordinator"] == coordinator_account, "wrong coordinator selected")
    exercise_project_brief(client, coordinator, worker, outsider, project_path)
    require(outsider.request("GET", "/api/projects") == [], "project list leaked another client's project")
    year, week_number, _ = (date.today() + timedelta(weeks=2)).isocalendar()
    week = {"isoYear": year, "week": week_number}
    window = {"start": week, "end": week}
    coordinator.command("POST", project_path + "/planning/quote", {"fee": "100", "minutes": 100, "window": window})
    project = client.request("GET", project_path)
    client.command("POST", project_path + "/planning/accept", {"expectedRevision": project["planning"]["revision"]})
    definition = {
        "title": "Implementation", "description": "One milestone",
        "milestones": [{"key": 1, "title": "Ship", "window": window,
                        "coordinatorFee": "100", "coordinatorMinutes": 60,
                        "requirements": [{"key": 1, "roleId": 2, "skillIds": [1, 5, 13],
                                          "minutes": 120, "budget": "900"}]}],
    }
    coordinator.command("POST", project_path + "/proposals", definition)
    project = client.request("GET", project_path)
    proposal = project["proposals"][0]
    proposal_path = project_path + "/proposals/" + proposal["proposalId"]
    milestone = proposal["milestones"][0]
    storage_path = project_path + "/task-storages/" + milestone["taskStorage"]["taskStorageId"]
    task = {"title": "Implement", "description": "Tracked separately from budget", "taskType": "Task",
            "priority": "Medium", "status": "ToDo", "assignees": [],
            "estimatedMinutes": 120, "loggedMinutes": 0, "dueAt": None}
    coordinator.command("POST", storage_path + "/tasks", task)
    coordinator.command("POST", proposal_path + "/submit")
    project = client.request("GET", project_path)
    client.command("POST", project_path + "/planning/accept-delivery", {"expectedRevision": project["planning"]["revision"]})
    require(coordinator.request("GET", "/api/balance")["available"] == "100", "planning fee not settled")
    project = client.request("GET", project_path)
    client.command("POST", proposal_path + "/approve", {"expectedRevision": project["proposals"][0]["revision"]})
    project = client.request("GET", project_path)
    milestone = project["proposals"][0]["milestones"][0]
    require(milestone["status"] == "InProgress", "execution did not start")
    require(milestone["assignments"] == [{"requirementKey": 1, "worker": worker_account}], "skills/mode assignment failed")
    require(worker.request("GET", project_path + "/brief") == client.request("GET", project_path + "/brief"),
            "assigned worker cannot read the client brief")
    return (client, coordinator, worker, password, coordinator_account, worker_account,
            project_id, project_path, storage_path, milestone, task)


def exercise(base, admin_password, proxy):
    (client, coordinator, worker, password, coordinator_account, worker_account,
     _project_id, project_path, storage_path, milestone, task) = prepare_single(base, admin_password)
    task_id = milestone["taskStorage"]["tasks"][0]["taskId"]
    task["assignees"] = [worker_account]
    coordinator.command("PUT", storage_path + "/tasks/" + str(task_id), task)
    worker.command("PATCH", storage_path + f"/tasks/{task_id}/progress", {"status": "Done", "loggedMinutes": 999})
    milestone_path = project_path + "/milestones/" + milestone["milestoneId"]
    submission = coordinator.command("POST", milestone_path + "/request-completion", {
        "deliverable": {"url": "https://example.test/delivery"},
    })
    acceptance = {"submissionId": submission["receipt"]["createdEntityId"]}
    key = "0x" + secrets.token_hex(16)
    proxy.drop_next = True
    first = client.command("POST", milestone_path + "/accept-completion", acceptance, key)
    replay = client.command("POST", milestone_path + "/accept-completion", acceptance, key)
    require(first == replay and proxy.dropped == 1, "lost-response recovery or idempotent replay failed")
    balances = [int(actor.request("GET", "/api/balance")["available"]) for actor in (client, coordinator, worker)]
    require(balances == [8900, 200, 900] and sum(balances) == 10000, "escrow payout conservation failed")
    project = client.request("GET", project_path)
    require(project["executionEscrow"] == "0", "settled funds left in escrow")
    directory = {entry["account"]: entry for entry in client.request("GET", "/api/workers")}
    require(directory[worker_account]["workerScore"] == {"weightedScoreSum": "0", "ratedMinutes": 0},
            "settlement must not fabricate a worker vote")
    require(directory[coordinator_account]["coordinatorScore"] == {"weightedScoreSum": "0", "ratedMinutes": 0},
            "settlement must not fabricate a coordinator vote")
    for account, committed in ((worker_account, 120), (coordinator_account, 160)):
        calendar = directory[account]["calendar"]
        require("reservations" not in calendar, "worker directory leaked reservation project identifiers")
        require(sum(item["minutes"] for item in calendar["committedMinutes"]) == committed,
                "completion restored consumed calendar capacity")
    before_account = worker.session["accountId"]
    worker.request("POST", "/api/auth/password", {"currentPassword": password, "newPassword": secrets.token_urlsafe(24)}, 204)
    require(worker.request("GET", "/api/auth/session")["accountId"] == before_account, "password change replaced wallet")
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        notifications = client.request("GET", "/api/notifications?after=0")["notifications"]
        if len(notifications) > 1:
            break
        time.sleep(0.1)
    require(len(notifications) > 1, "durable event ingestion produced no notifications")
    notification = client.first_sse_after(notifications[0]["notificationId"])
    replayed = client.first_sse_after(notifications[0]["notificationId"])
    require(replayed == notification and notification["readAt"] is None, "SSE replay changed unread state")
    client.request("POST", f"/api/notifications/{notification['notificationId']}/read")
    require(client.first_sse_after(notifications[0]["notificationId"])["readAt"] is not None,
            "explicit notification read was not persisted")
    client.request("POST", "/api/auth/logout", expected=204)
    client.request("GET", "/api/auth/session", expected=401)


def exercise_dispute(base, admin_password, _proxy):
    """Signed public flow: submit, reject, open, freeze, read and answer."""
    (client, coordinator, worker, _password, _coordinator_account, worker_account,
     project_id, project_path, _storage_path, milestone, _task) = prepare_single(base, admin_password)
    milestone_path = project_path + "/milestones/" + milestone["milestoneId"]
    reference = {"url": "https://example.test/dispute-evidence"}
    balances_before = [actor.request("GET", "/api/balance") for actor in (client, coordinator, worker)]
    submission = coordinator.command("POST", milestone_path + "/request-completion", {
        "deliverable": {"url": "https://example.test/delivery"},
    })
    submission_id = submission["receipt"]["createdEntityId"]
    client.command("POST", f"/api/completion-submissions/{submission_id}/rejection",
                   {"evidence": reference})
    changed = client.request("GET", project_path)
    changed_milestone = changed["proposals"][0]["milestones"][0]
    require(changed_milestone["status"] == "ChangesRequested", "rejection did not request changes")
    require(changed["activeDisputeId"] is None, "rejection opened a dispute implicitly")

    anonymous = Client(base)
    opening = {"projectId": project_id, "milestoneId": milestone["milestoneId"],
               "rejectedSubmissionId": submission_id, "reason": "Written dispute: ipfs://supporting-notes"}
    anonymous.request("POST", "/api/disputes", opening, 401)
    opened = client.command("POST", "/api/disputes", opening)
    dispute_id = opened["receipt"]["createdEntityId"]

    frozen = client.request("GET", project_path)
    frozen_milestone = frozen["proposals"][0]["milestones"][0]
    require(frozen["activeDisputeId"] == dispute_id, "project does not link its active dispute")
    require(frozen_milestone["status"] == "Disputed" and frozen_milestone["frozen"],
            "opening did not freeze the milestone")
    public = anonymous.request("GET", f"/api/disputes/{dispute_id}")
    require(set(public) == {"dispute", "milestone"}, "public case leaked unrelated state")
    require(public["dispute"]["rejectedSubmissionId"] == submission_id,
            "public case points to the wrong submission")
    require(public["dispute"]["response"] is None, "new case already has a response")

    blocked = client.command("POST", project_path + "/cancel", {"reason": "blocked"},
                             expected_outcome="Failed")
    require(blocked["receipt"]["outcome"]["code"] == "project_disputed",
            "project mutation bypassed the dispute freeze")
    presentation = anonymous.request("GET", f"/api/disputes/{dispute_id}/presentation")
    require(presentation["presentation"]["openingReason"] == opening["reason"], "written opening lost")
    for actor, kind in ((coordinator, "RESPONSE"), (client, "ADDITIONAL"), (coordinator, "RESPONSE")):
        payload = {"entryId": "0x" + secrets.token_hex(16), "content": "Written intervention", "argumentType": kind}
        endpoint = f"/api/disputes/{dispute_id}/arguments"
        saved = actor.request("POST", endpoint, payload, 201)
        require(actor.request("POST", endpoint, payload) == saved, "entry retry changed the record")
        actor.request("POST", endpoint, {**payload, "content": "edited"}, 409)
    arguments = anonymous.request("GET", f"/api/disputes/{dispute_id}/arguments")
    require(len(arguments["items"]) == 3, "multiple arguments were not retained")
    message = {"entryId": "0x" + secrets.token_hex(16), "content": "Private conversation", "argumentType": "MESSAGE"}
    client.request("POST", f"/api/disputes/{dispute_id}/messages", message, 201)
    require(len(coordinator.request("GET", f"/api/disputes/{dispute_id}/messages")["items"]) == 1,
            "counterparty cannot read the private channel")
    anonymous.request("GET", f"/api/disputes/{dispute_id}/messages", expected=401)
    worker.request("GET", f"/api/disputes/{dispute_id}/messages", expected=404)
    require(all(item["argumentType"] != "MESSAGE" for item in arguments["items"]), "private channel leaked")
    balances_after = [actor.request("GET", "/api/balance") for actor in (client, coordinator, worker)]
    require(balances_after == balances_before, "dispute opening or response moved funds")
def exercise_multi_milestone(base, admin_password, _proxy):
    """Legacy-sized teams, with the approved all-skills and weekly-capacity rules."""
    admin = Client(base)
    admin.authenticate("/api/auth/login", {"username": "admin", "password": admin_password})
    people = []
    password = secrets.token_urlsafe(24)
    for index in range(13):  # Client, two coordinators, five available and five unavailable workers.
        actor = Client(base, ORIGINS[index % 2])
        actor.authenticate("/api/auth/register", {
            "username": "teams" + secrets.token_hex(8), "password": password,
            "displayName": f"Team participant {index}",
        })
        people.append(actor)
    client = people[0]
    coordinators, workers, unavailable = people[1:3], people[3:8], people[8:13]
    for index, actor in enumerate(people[1:]):
        # Every worker has the common skill, but only its own slot's second skill.
        skills = list(range(1, 7)) if index < 2 else [1, 2 + (index - 2) % 5]
        actor.command("POST", "/api/workers", {
            "displayName": actor.session["displayName"],
            "qualifications": {"roleIds": [3], "skillIds": skills},
            "calendar": {"defaultWeeklyMinutes": 600 if index < 7 else 0, "overrides": []},
        })
    for actor in coordinators:
        admin.command("POST", "/api/admin/coordinators", {"account": actor.session["accountId"]})
        actor.command("PUT", "/api/workers/me/mode", {"mode": "Coordinator"})
    admin.command("POST", "/api/admin/fund", {"account": client.session["accountId"], "amount": "20000"})
    operation = client.command("POST", "/api/projects", {"title": "Four milestones", "description": "Teams 5/3/4/2"})
    project_path = "/api/projects/" + operation["receipt"]["createdEntityId"]
    project = client.request("GET", project_path)
    coordinator = next(actor for actor in coordinators if actor.session["accountId"] == project["coordinator"])
    teams = ((0, 1, 2, 3, 4), (0, 1, 2), (0, 1, 3, 4), (0, 2))
    windows = []
    for offset in range(4):
        year, week_number, _ = (date.today() + timedelta(weeks=2 + offset)).isocalendar()
        week = {"isoYear": year, "week": week_number}
        windows.append({"start": week, "end": week})
    coordinator.command("POST", project_path + "/planning/quote", {
        "fee": "100", "minutes": 100, "window": windows[0],
    })
    project = client.request("GET", project_path)
    client.command("POST", project_path + "/planning/accept", {"expectedRevision": project["planning"]["revision"]})
    definition = {"title": "Multi-team implementation", "description": "Four independently tracked milestones", "milestones": []}
    for index, team in enumerate(teams):
        definition["milestones"].append({
            "key": index + 1, "title": f"Milestone {index + 1}", "window": windows[index],
            "coordinatorFee": "100", "coordinatorMinutes": 30 * (index + 1),
            "requirements": [{"key": slot + 1, "roleId": 2, "skillIds": [1, slot + 2],
                              "minutes": 60 * (index + 1), "budget": "900"} for slot in team],
        })
    coordinator.command("POST", project_path + "/proposals", definition)
    project = client.request("GET", project_path)
    proposal = project["proposals"][0]
    proposal_path = project_path + "/proposals/" + proposal["proposalId"]
    storages = [item["taskStorage"]["taskStorageId"] for item in proposal["milestones"]]
    require(len(storages) == len(set(storages)) == 4, "milestones do not own four distinct task storages")
    definition["description"] = "Updated draft retains its milestone storages"
    coordinator.command("PUT", proposal_path, definition)
    edited = client.request("GET", project_path)["proposals"][0]
    require([item["taskStorage"]["taskStorageId"] for item in edited["milestones"]] == storages,
            "draft editing replaced milestone storages")
    for index, team in enumerate(teams):
        for slot in team:
            coordinator.command("POST", f"/api/task-storages/{storages[index]}/tasks", {
                "title": f"Slot {slot + 1}", "description": "Contractual time is independent of logged time",
                "taskType": "Task", "priority": "Medium", "status": "ToDo", "assignees": [],
                "estimatedMinutes": 60 * (index + 1), "loggedMinutes": 0, "dueAt": None,
            })
    direct_storage = coordinator.request("GET", f"/api/task-storages/{storages[0]}")
    require(len(direct_storage["tasks"]) == 5, "direct task-storage route disagrees with project")
    first_task = direct_storage["tasks"][0]
    first_task_path = f"/api/task-storages/{storages[0]}/tasks/{first_task['taskId']}"
    require(coordinator.request("GET", first_task_path) == first_task,
            "direct task read disagrees with storage")
    project_task = client.request("GET", project_path)["proposals"][0]["milestones"][0]["taskStorage"]["tasks"][0]
    require(project_task == first_task, "project read disagrees with direct task read")
    other_coordinator = next(actor for actor in coordinators if actor is not coordinator)
    other_coordinator.request("PUT", first_task_path, first_task["task"], expected=404)
    coordinator.command("POST", proposal_path + "/submit")
    project = client.request("GET", project_path)
    client.command("POST", project_path + "/planning/accept-delivery", {"expectedRevision": project["planning"]["revision"]})
    project = client.request("GET", project_path)
    client.command("POST", proposal_path + "/approve", {"expectedRevision": project["proposals"][0]["revision"]})
    project = client.request("GET", project_path)
    require(project["executionEscrow"] == "13000", "full four-milestone execution budget was not locked")
    milestones = project["proposals"][0]["milestones"]
    require([item["taskStorage"]["taskStorageId"] for item in milestones] == storages,
            "approval replaced milestone storages")
    require([len(item["assignments"]) for item in milestones] == [5, 3, 4, 2], "team sizes differ from 5/3/4/2")
    require([item["status"] for item in milestones] ==
            ["InProgress", "NotStarted", "NotStarted", "NotStarted"],
            "milestones did not activate sequentially")
    expected_minutes = [0] * 5
    expected_scores = [0] * 5
    expected_balances = [0] * 5
    escrow = 13000
    for index, (milestone, team) in enumerate(zip(milestones, teams)):
        expected = [{"requirementKey": slot + 1, "worker": workers[slot].session["accountId"]} for slot in team]
        require(milestone["assignments"] == expected, "all-skills, mode, capacity or distinct-slot matching failed")
        require(client.request("GET", project_path)["proposals"][0]["milestones"][index]["status"]
                == "InProgress", "current milestone did not start execution")
        tasks = milestone["taskStorage"]["tasks"]
        require(len(tasks) == len(team), "tasks leaked between milestone storages")
        for task, slot in zip(tasks, team):
            path = project_path + f"/task-storages/{storages[index]}/tasks/{task['taskId']}"
            definition = dict(task["task"])
            definition["assignees"] = [workers[slot].session["accountId"]]
            coordinator.command("PUT", path, definition)
            workers[slot].command("PATCH", path + "/progress", {"status": "Done", "loggedMinutes": 999})
        path = project_path + "/milestones/" + milestone["milestoneId"]
        submission = coordinator.command("POST", path + "/request-completion", {
            "deliverable": {"url": "https://example.test/delivery"},
        })
        requested = client.request("GET", project_path)["proposals"][0]["milestones"][index]
        require(requested["status"] == "CompletionRequested", "completion request was not recorded")
        require(client.request("GET", project_path)["executionEscrow"] == str(escrow), "request prematurely paid escrow")
        acceptance = {"submissionId": submission["receipt"]["createdEntityId"]}
        key = "0x" + secrets.token_hex(16)
        first = client.command("POST", path + "/accept-completion", acceptance, key)
        require(client.command("POST", path + "/accept-completion", acceptance, key) == first,
                "multi-milestone acceptance replay changed receipt")
        escrow -= 100 + 900 * len(team)
        for slot in team:
            expected_balances[slot] += 900
        project = client.request("GET", project_path)
        require(project["executionEscrow"] == str(escrow), "milestone settlement consumed the wrong escrow")
        require([item["status"] for item in project["proposals"][0]["milestones"]]
                == ["Completed"] * (index + 1)
                + (["InProgress"] if index < 3 else [])
                + ["NotStarted"] * max(0, 2 - index), "settlement changed milestone sequence")
        require(project["completed"] == (index == 3), "project completion did not follow final milestone")
        balances = [int(actor.request("GET", "/api/balance")["available"]) for actor in people]
        require(balances[0] == 6900 and sum(balances) + escrow == 20000, "multi-team token conservation failed")
        require(balances[3:8] == expected_balances and balances[8:] == [0] * 5, "wrong workers received payments")
        require(coordinator.request("GET", "/api/balance")["available"] == str(100 + 100 * (index + 1)),
                "coordinator fee missing or duplicated")
        directory = {entry["account"]: entry for entry in client.request("GET", "/api/workers")}
        for slot, worker in enumerate(workers):
            require(directory[worker.session["accountId"]]["workerScore"] == {
                "weightedScoreSum": str(expected_scores[slot]), "ratedMinutes": expected_minutes[slot]},
                "reputation did not accumulate committed minutes exactly once")
        require(directory[coordinator.session["accountId"]]["coordinatorScore"] == {
            "weightedScoreSum": "0", "ratedMinutes": 0},
            "coordinator reputation differs from accepted coordination minutes")
    for slot, worker in enumerate(workers):
        calendar = directory[worker.session["accountId"]]["calendar"]
        expected = [{"week": windows[index]["start"], "minutes": 60 * (index + 1)}
                    for index, team in enumerate(teams) if slot in team]
        require(calendar["committedMinutes"] == expected, "weekly reservations differ from contractual team assignments")
    for worker in unavailable:
        require(directory[worker.session["accountId"]]["calendar"]["committedMinutes"] == [],
                "unavailable worker was booked")
    other_coordinator = next(actor for actor in coordinators if actor is not coordinator)
    other = directory[other_coordinator.session["accountId"]]
    require(other["calendar"]["committedMinutes"] == [] and other["coordinatorScore"] == {
        "weightedScoreSum": "0", "ratedMinutes": 0}, "unselected coordinator acquired commitments or reputation")


def exercise_profiles_passkeys(base, owner, outsider, password):
    profile = {"name": "Example client", "company": "Example Ltd",
               "department": "Private department", "website": "https://example.test",
               "description": "Public summary", "location": "Madrid", "languages": ["es"]}
    owner.request("PUT", "/api/profiles/me", {"section": "client", "profile": profile})
    own = owner.request("GET", "/api/profiles/me")
    require(own["client"] == profile and own["worker"] is None, "owner profile was not persisted")
    public = Client(base).request("GET", "/api/profiles/" + owner.session["principalId"])
    require(public["client"]["company"] == profile["company"], "public client profile missing")
    require("department" not in public["client"] and "email" not in json.dumps(public)
            and "csrfToken" not in json.dumps(public), "public profile exposed private fields")
    require(outsider.request("GET", "/api/profiles/me")["client"] is None,
            "another principal read the owner's private profile")
    owner.request("PUT", "/api/profiles/me", {"section": "client", "profile": {
        **profile, "isAdmin": True}}, 422)

    image = bytes.fromhex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489"
        "0000000b49444154789c6300010000050001a5f645400000000049454e44ae426082")
    upload = urllib.request.Request(
        base + "/api/profiles/me/client/image", data=image, method="PUT",
        headers={"Origin": owner.origin, "X-CSRF-Token": owner.session["csrfToken"],
                 "Content-Type": "image/png"})
    with owner.opener.open(upload, timeout=10) as response:
        require(response.status == 204, "profile image upload failed")
    public_image = urllib.request.Request(
        base + "/api/profiles/" + owner.session["principalId"] + "/client/image")
    with Client(base).opener.open(public_image, timeout=10) as response:
        require(response.read() == image and response.headers.get("Content-Type") == "image/png",
                "public profile image disagrees with uploaded bytes")

    anonymous = Client(base)
    anonymous.request("POST", "/api/auth/passkeys/register/options",
                      {"currentPassword": password}, 401)
    owner.request("POST", "/api/auth/passkeys/register/options",
                  {"currentPassword": "incorrect-password"}, 401)
    options = owner.request("POST", "/api/auth/passkeys/register/options",
                            {"currentPassword": password})
    require(options["ceremonyId"] and options["options"], "passkey options were not issued")
    require(owner.request("GET", "/api/auth/passkeys") == [],
            "unverified passkey options created a credential")
    anonymous.request("POST", "/api/auth/passkeys/login/verify",
                      {"ceremonyId": "missing", "credential": {}}, (400, 401, 422))
    anonymous.request("GET", "/api/auth/session", expected=401)


def exercise_bramp(base, admin, owner, outsider):
    initial = int(owner.request("GET", "/api/balance")["available"])
    deposit = owner.command("POST", "/api/bramp/deposits", {"amount": "25"})
    deposit_id = deposit["receipt"]["createdEntityId"]
    path = "/api/bramp/deposits/" + deposit_id
    pending = owner.request("GET", path)
    require(pending["status"] == "Pending" and pending["owner"] == owner.session["accountId"]
            and pending["destination"] == pending["owner"] and pending["amount"] == "25",
            "deposit terms changed before confirmation")
    require(int(owner.request("GET", "/api/balance")["available"]) == initial,
            "pending deposit changed spendable balance")
    outsider.request("GET", path, expected=404)
    confirm_path = "/api/admin/bramp/deposits/" + deposit_id + "/confirm"
    outsider.request("POST", confirm_path, expected=403)
    key = "0x" + secrets.token_hex(16)
    first = admin.command("POST", confirm_path, operation_id=key)
    require(admin.command("POST", confirm_path, operation_id=key) == first,
            "deposit replay changed the receipt")
    repeated = admin.command("POST", confirm_path, expected_outcome="Failed")
    require(repeated["receipt"]["outcome"]["code"] == "deposit_already_confirmed",
            "a second deposit confirmation was not rejected")
    require(owner.request("GET", path)["status"] == "Confirmed"
            and int(owner.request("GET", "/api/balance")["available"]) == initial + 25,
            "deposit was not credited exactly once")

    excessive = owner.command("POST", "/api/bramp/withdrawals",
                              {"amount": str(initial + 26)}, expected_outcome="Failed")
    require(excessive["receipt"]["outcome"]["code"] == "insufficient_balance",
            "withdrawal spent more than the free balance")
    withdrawal = owner.command("POST", "/api/bramp/withdrawals", {"amount": "20"})
    withdrawal_id = withdrawal["receipt"]["createdEntityId"]
    withdrawal_path = "/api/bramp/withdrawals/" + withdrawal_id
    require(owner.request("GET", withdrawal_path)["status"] == "Pending"
            and int(owner.request("GET", "/api/balance")["available"]) == initial + 5,
            "pending withdrawal did not hold free KVN")
    outsider.request("GET", withdrawal_path, expected=404)
    outsider.request("POST", withdrawal_path + "/cancel", expected=404)
    owner.command("POST", withdrawal_path + "/cancel")
    repeated = owner.command("POST", withdrawal_path + "/cancel", expected_outcome="Failed")
    require(repeated["receipt"]["outcome"]["code"] == "withdrawal_already_cancelled"
            and owner.request("GET", withdrawal_path)["status"] == "Cancelled"
            and int(owner.request("GET", "/api/balance")["available"]) == initial + 25,
            "withdrawal cancellation did not restore the hold exactly once")


def exercise_catalog(admin, worker, outsider):
    worker.command("POST", "/api/workers", {
        "displayName": "Catalog worker", "qualifications": {"roleIds": [3], "skillIds": [1]},
        "calendar": {"defaultWeeklyMinutes": 600, "overrides": []},
    })
    catalog_before = worker.request("GET", "/api/catalog")
    skill_name = "Auxiliary skill " + secrets.token_hex(4)
    request = worker.command("POST", "/api/catalog/skill-requests",
                             {"name": skill_name, "roleIds": [3]})
    request_id = request["receipt"]["createdEntityId"]
    mine = worker.request("GET", "/api/catalog/skill-requests/me")
    require(len(mine) == 1 and mine[0]["requestId"] == request_id
            and mine[0]["status"] == "Pending", "skill request is not owner-visible")
    require(outsider.request("GET", "/api/catalog/skill-requests/me") == [],
            "another principal saw a private skill request")
    require(any(item["requestId"] == request_id for item in
                admin.request("GET", "/api/admin/catalog/skill-requests")),
            "operator cannot review the skill request")
    require(worker.request("GET", "/api/catalog")["skills"] == catalog_before["skills"],
            "pending request mutated the global catalog")
    decision_path = "/api/admin/catalog/skill-requests/" + request_id + "/decision"
    outsider.request("POST", decision_path, "Approve", 403)
    key = "0x" + secrets.token_hex(16)
    first = admin.command("POST", decision_path, "Approve", key)
    require(admin.command("POST", decision_path, "Approve", key) == first,
            "skill decision replay changed the receipt")
    repeated = admin.command("POST", decision_path, "Approve", expected_outcome="Failed")
    require(repeated["receipt"]["outcome"]["code"] == "skill_request_decided",
            "a decided request was approved twice")
    decided = worker.request("GET", "/api/catalog/skill-requests/me")[0]
    skill_id = decided["skillId"]
    catalog = worker.request("GET", "/api/catalog")
    require(decided["status"] == "Approved" and len(catalog["skills"]) == len(catalog_before["skills"]) + 1
            and sum(item["name"] == skill_name for item in catalog["skills"]) == 1
            and any(item["skillId"] == skill_id and item["roleIds"] == [3]
                    for item in catalog["skillRoles"]), "approval did not publish the skill once")
    directory = {item["account"]: item for item in worker.request("GET", "/api/workers")}
    require(skill_id not in directory[worker.session["accountId"]]["qualifications"]["skillIds"],
            "approval silently qualified the worker")
    duplicate = worker.command("POST", "/api/catalog/skill-requests",
                               {"name": "  " + skill_name.upper() + "  ", "roleIds": [3]},
                               expected_outcome="Failed")
    require(duplicate["receipt"]["outcome"]["code"] == "skill_name_exists",
            "normalized duplicate created a second global skill")


def exercise_auxiliary(base, admin_password, _proxy):
    """Typed REST checks beyond the original project and dispute paths."""
    admin = Client(base)
    admin.authenticate("/api/auth/login", {"username": "admin", "password": admin_password})
    password = secrets.token_urlsafe(24)
    owner, worker, outsider = (Client(base) for _ in range(3))
    for actor in (owner, worker, outsider):
        actor.authenticate("/api/auth/register", {
            "username": "aux" + secrets.token_hex(8), "password": password,
            "displayName": "Auxiliary flow",
        })
    exercise_profiles_passkeys(base, owner, outsider, password)
    exercise_bramp(base, admin, owner, outsider)
    exercise_catalog(admin, worker, outsider)


def scan_secret_logs(logs):
    for service, log in logs:
        log.flush()
        log.seek(0)
        output = log.read(4 * 1024 * 1024 + 1)
        require(len(output) <= 4 * 1024 * 1024, f"{service} log exceeds secret-scan limit")
        for name, marker in Client.secret_markers:
            require(marker not in output,
                    f"{service} log disclosed {name}")


def run(storage, binaries, scenario=exercise):
    with ExitStack() as stack:
        directory = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="kunveno-e2e-")))
        logs = []
        adapter_database_url = temporary_postgres(stack, directory)
        secret_dir = directory / "secrets"
        subprocess.run([str(binaries / "wallet"), "init-dev-secrets", str(secret_dir)],
                       check=True, stdout=subprocess.DEVNULL)
        Client.secret_markers = tuple(
            (name, (secret_dir / name).read_bytes().strip()) for name in
            ("service-token", "master-key.hex", "root-seed.hex", "bootstrap-admin-password")
        )
        require(all(len(marker) >= 12 for _, marker in Client.secret_markers),
                "secret marker too short for reliable scanning")
        wallet_port, provider_port, adapter_port = [available_port() for _ in range(3)]
        common = {"INTERNAL_SERVICE_TOKEN_FILE": str(secret_dir / "service-token")}
        wallet = launch(stack, binaries / "wallet", wallet_port, {
            **common, "CUSTODY_DATABASE_URL": f"sqlite://{directory}/custody.sqlite?mode=rwc",
            "CUSTODY_MASTER_KEY_FILE": str(secret_dir / "master-key.hex"),
            "CUSTODY_ROOT_SEED_FILE": str(secret_dir / "root-seed.hex"),
        }, logs)
        launch(stack, binaries / "mock-provider", provider_port, {
            **common, "MOCK_DATABASE_URL": f"sqlite://{directory}/mock.sqlite?mode=rwc",
            "MOCK_STORAGE": storage, "MOCK_ROOT_ACCOUNT_FILE": str(secret_dir / "root-account.hex"),
        }, logs)
        proxy = LossyProvider(provider_port)
        proxy_thread = threading.Thread(target=proxy.serve_forever, daemon=True)
        proxy_thread.start()
        stack.callback(proxy.server_close)
        stack.callback(proxy.shutdown)
        adapter = launch(stack, binaries / "adapter-api", adapter_port, {
            **common, "ADAPTER_DATABASE_URL": adapter_database_url,
            "CUSTODY_URL": f"http://127.0.0.1:{wallet_port}",
            "MOCK_PROVIDER_URL": f"http://127.0.0.1:{proxy.server_port}",
            "ALLOWED_ORIGINS": ",".join(ORIGINS), "COOKIE_SECURE": "false",
            "ENABLE_MOCK_FUNDING": "true",
            "BOOTSTRAP_ADMIN_USERNAME": "admin",
            "BOOTSTRAP_ADMIN_PASSWORD_FILE": str(secret_dir / "bootstrap-admin-password"),
            "OPENAPI_PATH": str(ROOT / "contracts/openapi.json"),
        }, logs)
        scenario(adapter.base, (secret_dir / "bootstrap-admin-password").read_text().strip(), proxy)
        wallet.request("GET", "/internal/metrics", headers={
            "Authorization": "Bearer " + (secret_dir / "service-token").read_text().strip(),
        })
        scan_secret_logs(logs)
        print(f"PASS {storage} {scenario.__name__}: signed REST lifecycle")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--storage", choices=("sqlite", "memory", "both"), default="both")
    parser.add_argument("--binaries", type=Path, default=ROOT / "target/debug")
    arguments = parser.parse_args()
    for selected in (("sqlite", "memory") if arguments.storage == "both" else (arguments.storage,)):
        run(selected, arguments.binaries.resolve())
        run(selected, arguments.binaries.resolve(), exercise_multi_milestone)
        run(selected, arguments.binaries.resolve(), exercise_dispute)
        run(selected, arguments.binaries.resolve(), exercise_auxiliary)
