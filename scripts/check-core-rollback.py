#!/usr/bin/env python3
"""Rehearse R2a rollback with frozen predecessor binaries and disposable fixtures.

Generate the case with tools/ink-sandbox/tests/core_metadata.rs and
R2A_ROLLBACK_OUTPUT_DIR, then supply the retained 84ab1e4 binaries. This runs no
wallet, accesses no application database and performs no live cutover.
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

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("poc_e2e", ROOT / "scripts/poc-e2e.py")
POC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POC)


def load(path):
    with path.open() as source:
        return json.load(source)


def cli(binary, args, environment, marker):
    result = subprocess.run([str(binary), *map(str, args)],
                            env={**os.environ, **environment}, capture_output=True, timeout=120)
    POC.require(marker not in result.stdout and marker not in result.stderr,
                "predecessor CLI disclosed the disposable service token")
    POC.require(result.returncode == 0, "predecessor CLI failed; rollback not verified")


def run(case, binaries):
    binary = binaries / "whole-provider"
    expected = load(case / "predecessor.json")
    calls, receipts, next_calls = (load(case / name) for name in ("calls.json", "receipts.json", "next.json"))
    POC.require(expected["version"] == 1 and expected["checkpoint"]["engine_version"] == 1,
                "expected an explicit predecessor cohort")
    print("Predecessor whole-provider SHA-256:", hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)
    for authority in ("rust", "shadow", "sandbox"):
        with ExitStack() as resources:
            directory = Path(resources.enter_context(tempfile.TemporaryDirectory(prefix="r2a-rollback-")))
            token = secrets.token_urlsafe(40).encode()
            token_file = directory / "service-token"
            token_file.write_bytes(token)
            token_file.chmod(0o600)
            environment = {
                "WHOLE_DATABASE_PATH": str(directory / "predecessor.sqlite"),
                "PROVIDER_AUTHORITY": authority,
                "MOCK_ROOT_ACCOUNT_FILE": str(case / "root-account.hex"),
                "INTERNAL_SERVICE_TOKEN_FILE": str(token_file),
            }
            POC.Client.secret_markers = (("disposable service token", token),)
            authorization = {"Authorization": "Bearer " + token.decode()}
            cli(binary, ["import", case / "predecessor.json"], environment, token)
            for phase in ("first", "reopened"):
                with ExitStack() as serving:
                    logs = []
                    client = POC.launch(serving, binary, POC.available_port(), environment, logs)
                    snapshot = client.request("GET", "/internal/snapshot", headers=authorization)
                    saved = expected["checkpoint"]
                    POC.require(snapshot["info"] == saved["info"], "rollback changed generation or root")
                    for collection in ("workers", "projects"):
                        wanted = [value for _, value in sorted(saved[collection].items())]
                        POC.require(snapshot[collection] == wanted, "rollback changed current " + collection)
                    for call, receipt in zip(calls, receipts, strict=True):
                        replay = client.request("POST", "/internal/contracts/call", call, headers=authorization)
                        POC.require(replay == receipt, "compact receipt changed on predecessor replay")
                    if phase == "reopened":
                        for call in next_calls:
                            result = client.request("POST", "/internal/contracts/call", call, headers=authorization)
                            POC.require(result["outcome"]["type"] == "Success", "ordinary predecessor work failed")
                            POC.require(client.request("POST", "/internal/contracts/call", call, headers=authorization) == result,
                                        "further work was not idempotent")
                    POC.scan_secret_logs(logs)
                output = directory / (phase + ".json")
                cli(binary, ["export", output], environment, token)
                observed = load(output)
                if phase == "first":
                    POC.require(observed == expected, "replay/import changed complete predecessor state")
                else:
                    before, after = expected["checkpoint"], observed["checkpoint"]
                    POC.require(after["balances"] == before["balances"] and after["minted_units"] == before["minted_units"],
                                "further work changed funds")
                    POC.require(after["workers"] == before["workers"] and after["next_task_id"] == before["next_task_id"],
                                "further work changed worker facts or task counters")
                    POC.require(len(after["projects"]) == len(before["projects"]) + 1,
                                "ordinary creation did not create exactly one Project")
                    POC.require(len(after["receipts"]) == len(before["receipts"]) + len(next_calls)
                                and len(after["events"]) == len(before["events"]) + len(next_calls),
                                "further work changed receipt/event accounting")
                    for operation, receipt in before["receipts"].items():
                        POC.require(after["receipts"][operation] == receipt, "retained call bytes or receipt changed")
            print("PASS", authority, "current-state rollback: compact success, exact replay, restart and further legacy work", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", required=True, type=Path)
    parser.add_argument("--predecessor-binaries", required=True, type=Path)
    arguments = parser.parse_args()
    run(arguments.case.resolve(), arguments.predecessor_binaries.resolve())
