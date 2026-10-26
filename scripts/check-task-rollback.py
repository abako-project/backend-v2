#!/usr/bin/env python3
"""Rehearse API-current tasks → frozen native provider using disposable stores.

Task identity and membership use real core transactions. Text/progress use API
results. The real worker, provider, wallet, API, notifications and paused owner
tools perform the handoff; no operation rows or ownership flags are fabricated.
Supply frozen, hash-verified predecessor binaries; never rebuild them in place.
"""
import argparse
from contextlib import ExitStack
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("poc_e2e", ROOT / "scripts/poc-e2e.py")
POC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POC)
SPEC = importlib.util.spec_from_file_location("core_rollback", ROOT / "scripts/check-core-rollback.py")
CORE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CORE)
require, cli = POC.require, CORE.cli


def sql(database, statement):
    return subprocess.run(["psql", database, "-XqAt", "-v", "ON_ERROR_STOP=1", "-c", statement],
                          check=True, capture_output=True, timeout=30).stdout.decode().strip()


def literal(value):
    return "NULL" if value is None else "'" + str(value).replace("'", "''") + "'"


def load(path):
    return json.loads(path.read_bytes())


def history(database):
    # All history rows, including read state, immutable freeze snapshots, source
    # bytes and task payloads. Only the final ownership flag may change.
    return tuple(sql(database, f"SELECT row_to_json(t)::text FROM {table} t ORDER BY {order}")
                 for table, order in (("operations", "creation_sequence"), ("notifications", "notification_id"),
                                      ("task_migration_cases", "case_id"),
                                      ("task_items", "task_id"), ("task_storages", "task_storage_id"),
                                      ("task_freezes", "operation_id")))


def task_command(actor, method, path, body, expected="Success", membership=False):
    operation = actor.command(method, path, body, expected_outcome=expected)
    core = (method in ("POST", "DELETE") or membership) and expected == "Success"
    require((operation.get("receipt") is not None) == core,
            "task returned the wrong authority's result")
    return operation["operationId"], operation


def run(binaries, predecessor, sdk=None, authority="sandbox"):
    manifest = load(predecessor / "manifest.json")
    records = manifest.get("binaries", manifest.get("files"))
    require(isinstance(records, dict), "missing frozen binary manifest")
    for name in ("adapter-api", "wallet", "mock-provider"):
        digest = hashlib.sha256((predecessor / name).read_bytes()).hexdigest()
        require(digest == records[name]["sha256"], "frozen predecessor hash changed")
        print(name + " predecessor SHA-256: " + digest, flush=True)
    current_provider = sdk or binaries / "mock-provider"
    previous_provider = predecessor / ("whole-provider" if sdk else "mock-provider")
    if sdk:
        digest = hashlib.sha256(previous_provider.read_bytes()).hexdigest()
        require(digest == manifest["sdkBinary"]["sha256"], "frozen SDK predecessor hash changed")
        print("whole-provider predecessor SHA-256: " + digest, flush=True)
    with ExitStack() as resources:
        directory = Path(resources.enter_context(tempfile.TemporaryDirectory(prefix="task-rollback-")))
        database = POC.temporary_postgres(resources, directory)
        secrets_dir = directory / "secrets"
        subprocess.run([str(binaries / "wallet"), "init-dev-secrets", str(secrets_dir)],
                       check=True, stdout=subprocess.DEVNULL)
        POC.Client.secret_markers = tuple((name, (secrets_dir / name).read_bytes().strip()) for name in
                                        ("service-token", "master-key.hex", "root-seed.hex", "bootstrap-admin-password"))
        common = {"INTERNAL_SERVICE_TOKEN_FILE": str(secrets_dir / "service-token")}
        wallet_env = {**common, "CUSTODY_DATABASE_URL": f"sqlite://{directory}/custody.sqlite?mode=rwc",
                      "CUSTODY_MASTER_KEY_FILE": str(secrets_dir / "master-key.hex"),
                      "CUSTODY_ROOT_SEED_FILE": str(secrets_dir / "root-seed.hex")}
        provider_env = {**common, "MOCK_STORAGE": "sqlite", "MOCK_DATABASE_URL": f"sqlite://{directory}/source.sqlite?mode=rwc",
                        "MOCK_ROOT_ACCOUNT_FILE": str(secrets_dir / "root-account.hex")}
        provider_env.update(WHOLE_DATABASE_PATH=str(directory / "source-whole.sqlite"), PROVIDER_AUTHORITY=authority)
        logs = []
        source = resources.enter_context(ExitStack())
        wallet = POC.launch(source, binaries / "wallet", POC.available_port(), wallet_env, logs)
        provider = POC.launch(source, current_provider, POC.available_port(), provider_env, logs)
        adapter_env = {**common, "ADAPTER_DATABASE_URL": database, "CUSTODY_URL": wallet.base,
                       "MOCK_PROVIDER_URL": provider.base, "ALLOWED_ORIGINS": ",".join(POC.ORIGINS),
                       "COOKIE_SECURE": "false", "ENABLE_MOCK_FUNDING": "true", "BOOTSTRAP_ADMIN_USERNAME": "admin",
                       "BOOTSTRAP_ADMIN_PASSWORD_FILE": str(secrets_dir / "bootstrap-admin-password"),
                       "OPENAPI_PATH": str(ROOT / "contracts/openapi.json")}
        adapter = POC.launch(source, binaries / "adapter-api", POC.available_port(), adapter_env, logs)
        (client, coordinator, worker, _, _, worker_account, project_id, project_path,
         storage_path, milestone, task) = POC.prepare_single(adapter.base, (secrets_dir / "bootstrap-admin-password").read_text().strip())
        created = client.command("POST", "/api/projects", {"title": "Rollback draft", "description": "Deletion gap"})
        draft_id = created["receipt"]["createdEntityId"]
        draft_path = "/api/projects/" + draft_id
        year, week, _ = (POC.date.today() + POC.timedelta(weeks=2)).isocalendar()
        window = {"start": {"isoYear": year, "week": week}, "end": {"isoYear": year, "week": week}}
        definition = {"title": "Draft", "description": "Preserve API task data", "milestones": [
            {"key": 1, "title": "Draft task storage", "window": window,
             "coordinatorFee": "10", "coordinatorMinutes": 60, "requirements": [
                 {"key": 1, "roleId": 2, "skillIds": [1, 5, 13], "minutes": 120, "budget": "90"}]}]}
        coordinator.command("POST", draft_path + "/proposals", definition)
        draft = client.request("GET", draft_path)
        draft_storage = draft_path + "/task-storages/" + draft["proposals"][0]["milestones"][0]["taskStorage"]["taskStorageId"]
        generation = sql(database, "SELECT provider_instance_id FROM operations WHERE provider_instance_id IS NOT NULL LIMIT 1")
        outcomes = []

        def mutate(actor, method, path, body=None, project=draft_id, expected="Success", membership=False):
            record = task_command(actor, method, path, body, expected, membership)
            outcomes.append((actor, *record))
            return record

        mutate(coordinator, "POST", draft_storage + "/tasks", task)
        revision = draft["proposals"][0]["revision"]
        mutate(coordinator, "DELETE", draft_storage + "/tasks/2", {"expectedRevision": revision})
        task = {**task, "assignees": [worker_account], "description": "λ\0" * 20_000}
        mutate(coordinator, "POST", draft_storage + "/tasks", task)
        mutate(coordinator, "PUT", storage_path + "/tasks/1", {**task, "title": "Confirmed member"}, project_id, membership=True)
        before_path = directory / "before-local-edits.json"
        cli(current_provider, ["export", before_path], provider_env)
        mutate(coordinator, "PUT", draft_storage + "/tasks/3", {**task, "title": "Edited API task"})
        mutate(worker, "PATCH", draft_storage + "/tasks/3/progress", {"status": "InProgress", "loggedMinutes": 17})
        mutate(coordinator, "PUT", storage_path + "/tasks/1", {**task, "title": "Frozen API task"}, project_id)
        after_path = directory / "after-task-edits.json"
        cli(current_provider, ["export", after_path], provider_env)
        require(load(before_path) == load(after_path), "API tasks changed provider funds/reputation/nonces/receipts/events")
        milestone_path = project_path + "/milestones/" + milestone["milestoneId"]
        submission = coordinator.command("POST", milestone_path + "/request-completion", {"deliverable": {"url": "https://example.test/delivery"}})
        submission_id = submission["receipt"]["createdEntityId"]
        client.command("POST", "/api/completion-submissions/" + submission_id + "/rejection", {"evidence": {"url": "https://example.test/evidence"}})
        opened = client.command("POST", "/api/disputes", {"projectId": project_id, "milestoneId": milestone["milestoneId"],
                              "rejectedSubmissionId": submission_id, "reason": "Immutable task snapshot"})
        case_path = "/api/disputes/" + opened["receipt"]["createdEntityId"]
        mutate(worker, "PATCH", storage_path + "/tasks/1/progress", {"status": "Done", "loggedMinutes": 99}, project_id, "Failed")
        time.sleep(1)  # let real provider ingestion catch up before comparing history
        case = client.request("GET", case_path)
        views = [client.request("GET", path) for path in (project_path, draft_path)]
        notifications = worker.request("GET", "/api/notifications?after=0")["notifications"]
        require(len(notifications) >= 2, "no mixed notification history")
        cursor = notifications[0]["notificationId"]
        replay = worker.first_sse_after(cursor)
        worker.request("POST", f"/api/notifications/{replay['notificationId']}/read")
        replay = worker.first_sse_after(cursor)
        require(replay["readAt"] is not None, "read state missing")
        POC.scan_secret_logs(logs)
        source.close()  # stop ingress/workers/provider before the owner handoff
        # Export needs only a paused provider read service, not a running API worker.
        with ExitStack() as paused:
            export_logs = []
            provider = POC.launch(paused, current_provider, POC.available_port(), provider_env, export_logs)
            adapter_env["MOCK_PROVIDER_URL"] = provider.base
            tasks_path = directory / "current-tasks.json"
            cli(binaries / "adapter-api", ["export-current-tasks", generation, tasks_path], adapter_env)
            POC.scan_secret_logs(export_logs)
        tasks = load(tasks_path)
        require(tasks["nextTaskId"] == 4, "deleted task gap/counter was lost")
        require(sorted(t["taskId"] for r in tasks["storages"] for t in r["storage"]["tasks"]) == [1, 3], "wrong live tasks")
        core_path, restored_path = directory / "current-core.json", directory / "restored-core.json"
        cli(current_provider, ["export", core_path], provider_env)
        cli(binaries / "mock-provider", ["restore-tasks", core_path, tasks_path, restored_path], provider_env)
        retained = history(database)
        target_env = {**provider_env, "MOCK_DATABASE_URL": f"sqlite://{directory}/restored.sqlite?mode=rwc"}
        target_env["WHOLE_DATABASE_PATH"] = str(directory / "restored-whole.sqlite")
        arguments = ["import", restored_path] if sdk else ["import", restored_path, directory / "restored.sqlite"]
        cli(previous_provider, arguments, target_env)
        readback = directory / "target-readback.json"
        cli(previous_provider, ["export", readback], target_env)
        require(load(readback) == load(restored_path), "fresh target changed non-task state or history")
        for phase in ("first", "reopened"):
            with ExitStack() as target:
                target_logs = []
                provider = POC.launch(target, previous_provider, POC.available_port(), target_env, target_logs)
                wallet = POC.launch(target, predecessor / "wallet", POC.available_port(), wallet_env, target_logs)
                target_adapter_env = {**adapter_env, "MOCK_PROVIDER_URL": provider.base, "CUSTODY_URL": wallet.base}
                cli(binaries / "adapter-api", ["complete-task-rollback", tasks_path], target_adapter_env)
                require(history(database) == retained, "handoff rewrote API operation/notification/task/freeze history")
                adapter = POC.launch(target, predecessor / "adapter-api", POC.available_port(), target_adapter_env, target_logs)
                for actor in (client, coordinator, worker):
                    actor.base = adapter.base
                require([client.request("GET", path) for path in (project_path, draft_path)] == views, "public task views changed")
                require(client.request("GET", case_path) == case, "frozen public tasks changed")
                for actor, key, operation in outcomes:
                    require(actor.request("GET", "/api/operations/" + key) == operation, "task result history changed")
                require(worker.first_sse_after(cursor) == replay, "mixed SSE resume/read state changed")
                require(client.command("POST", "/api/disputes", {"projectId": project_id, "milestoneId": milestone["milestoneId"],
                               "rejectedSubmissionId": submission_id, "reason": "Immutable task snapshot"}, opened["operationId"]) == opened,
                        "existing signed operation replay changed")
                if phase == "reopened":
                    new = coordinator.command("POST", draft_storage + "/tasks", task)
                    require(new["receipt"] is not None and new.get("taskResult") is None, "provider task did not use truthful signing receipt")
                    current = coordinator.request("GET", "/api/task-storages/" + draft_storage.rsplit("/", 1)[1])
                    require([t["taskId"] for t in current["tasks"]] == [3, 4], "provider reused a deleted task ID")
                    coordinator.command("PUT", draft_storage + "/tasks/4", {**task, "title": "Provider edit after rollback"})
                    worker.command("PATCH", draft_storage + "/tasks/4/progress", {"status": "Done", "loggedMinutes": 23})
                    coordinator.command("DELETE", draft_storage + "/tasks/3", {"expectedRevision": revision})
                    require([t["taskId"] for t in coordinator.request("GET", "/api/task-storages/" + draft_storage.rsplit("/", 1)[1])["tasks"]] == [4], "further provider CRUD failed")
                POC.scan_secret_logs(target_logs)
        final_path = directory / "final-target.json"
        cli(previous_provider, ["export", final_path], target_env)
        final, original = load(final_path)["checkpoint"], load(restored_path)["checkpoint"]
        require(final["next_task_id"] == 5, "further work lost retained allocator")
        require(all(final["receipts"].get(key) == value for key, value in original["receipts"].items()), "accepted call bytes/receipts changed")
        require(final["events"][:len(original["events"])] == original["events"], "retained provider events changed")
        for checkpoint in (final, original):
            for key in ("next_task_id", "nonces", "receipts", "events"):
                checkpoint.pop(key)
            for project in checkpoint["projects"].values():
                for proposal in project["proposals"]:
                    for milestone in proposal["milestones"]:
                        milestone["taskStorage"]["tasks"] = []
        require(final == original, "further task work changed funds, reputation or other non-task business facts")
        print(f"PASS: {authority if sdk else 'native'} paired current-task rollback, preserved history/frozen views, restart, replay and further signed CRUD", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", required=True, type=Path)
    parser.add_argument("--predecessor", required=True, type=Path)
    parser.add_argument("--sdk", type=Path, help="current whole-provider binary; otherwise use native provider")
    parser.add_argument("--authority", choices=("rust", "shadow", "sandbox"), default="sandbox")
    args = parser.parse_args()
    run(args.binaries.resolve(), args.predecessor.resolve(), args.sdk.resolve() if args.sdk else None, args.authority)
