#!/usr/bin/env python3
"""Exercise real custodial REST operations against disposable local services.

Run after `cargo build --workspace --all-features --locked`. No third-party
Python packages, Docker, existing users, or real tokens are required.
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


class Client:
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


def launch(stack, binary, port, environment):
    log = stack.enter_context(tempfile.TemporaryFile())
    process = subprocess.Popen(
        [str(binary)], cwd=ROOT, stdout=log, stderr=log,
        env={**os.environ, **environment, "BIND_ADDR": f"127.0.0.1:{port}"},
    )
    stack.callback(stop_process, process)
    client = Client(f"http://127.0.0.1:{port}")
    wait_ready(client, process)
    return client


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
        "workerRatings": [{"worker": worker_account, "score": 8}],
        "deliverable": {"url": "https://example.test/delivery", "sha256": "0x" + "07" * 32},
    })
    acceptance = {"coordinatorScore": 9, "teamRating": {"type": "Client", "score": 6},
                  "submissionId": submission["receipt"]["createdEntityId"]}
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
    require(directory[worker_account]["workerScore"] == {"weightedScoreSum": "84000", "ratedMinutes": 120},
            "worker score did not use committed minutes and 50/50 weights")
    require(directory[coordinator_account]["coordinatorScore"] == {"weightedScoreSum": "54000", "ratedMinutes": 60},
            "coordinator score did not use its separate committed duration")
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
    reference = {"url": "https://example.test/dispute-evidence", "sha256": "0x" + "09" * 32}
    balances_before = [actor.request("GET", "/api/balance") for actor in (client, coordinator, worker)]
    submission = coordinator.command("POST", milestone_path + "/request-completion", {
        "workerRatings": [{"worker": worker_account, "score": 8}],
        "deliverable": {"url": "https://example.test/delivery", "sha256": "0x" + "07" * 32},
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
               "rejectedSubmissionId": submission_id, "evidence": reference}
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
    coordinator.command("POST", f"/api/disputes/{dispute_id}/response", {"evidence": reference})
    answered = anonymous.request("GET", f"/api/disputes/{dispute_id}")
    require(answered["dispute"]["response"]["author"] == coordinator.session["accountId"],
            "counterparty response was not published")
    duplicate = coordinator.command("POST", f"/api/disputes/{dispute_id}/response",
                                    {"evidence": reference}, expected_outcome="Failed")
    require(duplicate["receipt"]["outcome"]["code"] == "dispute_already_answered",
            "case accepted a second response")
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
            coordinator.command("POST", project_path + f"/task-storages/{storages[index]}/tasks", {
                "title": f"Slot {slot + 1}", "description": "Contractual time is independent of logged time",
                "taskType": "Task", "priority": "Medium", "status": "ToDo", "assignees": [],
                "estimatedMinutes": 60 * (index + 1), "loggedMinutes": 0, "dueAt": None,
            })
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
    expected_minutes = [0] * 5
    expected_scores = [0] * 5
    expected_balances = [0] * 5
    escrow = 13000
    for index, (milestone, team) in enumerate(zip(milestones, teams)):
        expected = [{"requirementKey": slot + 1, "worker": workers[slot].session["accountId"]} for slot in team]
        require(milestone["assignments"] == expected, "all-skills, mode, capacity or distinct-slot matching failed")
        require(milestone["status"] == "InProgress", "approved milestone did not start execution")
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
            "workerRatings": [{"worker": workers[slot].session["accountId"], "score": 8} for slot in team],
            "deliverable": {"url": "https://example.test/delivery", "sha256": "0x" + "07" * 32},
        })
        requested = client.request("GET", project_path)["proposals"][0]["milestones"][index]
        require(requested["status"] == "CompletionRequested", "completion request was not recorded")
        require(client.request("GET", project_path)["executionEscrow"] == str(escrow), "request prematurely paid escrow")
        acceptance = {"submissionId": submission["receipt"]["createdEntityId"],
                      "coordinatorScore": 9, "teamRating": (
                          {"type": "DelegateToCoordinator"} if index == 3 else {"type": "Client", "score": 6})}
        key = "0x" + secrets.token_hex(16)
        first = client.command("POST", path + "/accept-completion", acceptance, key)
        require(client.command("POST", path + "/accept-completion", acceptance, key) == first,
                "multi-milestone acceptance replay changed receipt")
        escrow -= 100 + 900 * len(team)
        for slot in team:
            minutes = 60 * (index + 1)
            expected_minutes[slot] += minutes
            expected_scores[slot] += minutes * (800 if index == 3 else 700)
            expected_balances[slot] += 900
        project = client.request("GET", project_path)
        require(project["executionEscrow"] == str(escrow), "milestone settlement consumed the wrong escrow")
        require([item["status"] for item in project["proposals"][0]["milestones"]]
                == ["Completed"] * (index + 1) + ["InProgress"] * (3 - index), "settlement changed other milestone states")
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
        coordinator_minutes = sum(30 * (number + 1) for number in range(index + 1))
        require(directory[coordinator.session["accountId"]]["coordinatorScore"] == {
            "weightedScoreSum": str(900 * coordinator_minutes), "ratedMinutes": coordinator_minutes},
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


def run(storage, binaries, scenario=exercise):
    with ExitStack() as stack:
        directory = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="kunveno-e2e-")))
        secret_dir = directory / "secrets"
        subprocess.run([str(binaries / "wallet"), "init-dev-secrets", str(secret_dir)],
                       check=True, stdout=subprocess.DEVNULL)
        wallet_port, provider_port, adapter_port = [available_port() for _ in range(3)]
        common = {"INTERNAL_SERVICE_TOKEN_FILE": str(secret_dir / "service-token")}
        launch(stack, binaries / "wallet", wallet_port, {
            **common, "CUSTODY_DATABASE_URL": f"sqlite://{directory}/custody.sqlite?mode=rwc",
            "CUSTODY_MASTER_KEY_FILE": str(secret_dir / "master-key.hex"),
            "CUSTODY_ROOT_SEED_FILE": str(secret_dir / "root-seed.hex"),
        })
        launch(stack, binaries / "mock-provider", provider_port, {
            **common, "MOCK_DATABASE_URL": f"sqlite://{directory}/mock.sqlite?mode=rwc",
            "MOCK_STORAGE": storage, "MOCK_ROOT_ACCOUNT_FILE": str(secret_dir / "root-account.hex"),
        })
        proxy = LossyProvider(provider_port)
        proxy_thread = threading.Thread(target=proxy.serve_forever, daemon=True)
        proxy_thread.start()
        stack.callback(proxy.server_close)
        stack.callback(proxy.shutdown)
        adapter = launch(stack, binaries / "adapter-api", adapter_port, {
            **common, "ADAPTER_DATABASE_URL": f"sqlite://{directory}/adapter.sqlite?mode=rwc",
            "CUSTODY_URL": f"http://127.0.0.1:{wallet_port}",
            "MOCK_PROVIDER_URL": f"http://127.0.0.1:{proxy.server_port}",
            "ALLOWED_ORIGINS": ",".join(ORIGINS), "COOKIE_SECURE": "false",
            "BOOTSTRAP_ADMIN_USERNAME": "admin",
            "BOOTSTRAP_ADMIN_PASSWORD_FILE": str(secret_dir / "bootstrap-admin-password"),
            "OPENAPI_PATH": str(ROOT / "contracts/openapi.json"),
        })
        scenario(adapter.base, (secret_dir / "bootstrap-admin-password").read_text().strip(), proxy)
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
