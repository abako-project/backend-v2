#!/usr/bin/env python3
"""Rehearse the current V4→V3 catalog/reason rollback with disposable services.

Generate a case with the real Sandbox catalog_metadata test and
R2B2_ROLLBACK_OUTPUT_DIR. Supply the current and frozen d215b65 binaries.
This performs no live cutover; PostgreSQL, custody/provider stores and secrets
are disposable. Only owners' CLI/HTTP interfaces transfer service data.
"""
import argparse
from contextlib import ExitStack
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("poc_e2e", ROOT / "scripts/poc-e2e.py")
POC = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POC)


def load(path):
    return json.loads(path.read_bytes())


def cli(binary, args, environment):
    result = subprocess.run([str(binary), *map(str, args)],
                            env={**os.environ, **environment}, capture_output=True, timeout=120)
    for name, marker in POC.Client.secret_markers:
        POC.require(marker not in result.stdout and marker not in result.stderr,
                    "owner CLI disclosed " + name)
    if result.returncode != 0:
        # Test identities and secret markers were checked above. Preserve the
        # actual CLI error without inferring a capacity threshold or fallback.
        error = next((line for line in reversed(result.stderr.decode(errors="replace").splitlines())
                      if line.startswith("Error:")), "no typed CLI error")[:200]
        raise AssertionError(f"owner CLI failed: {binary.name} {args[0]} ({error}); rollback not verified")


def proposal_rows(database_url):
    records = []
    for table in ("proposal_documents", "catalog_documents", "lifecycle_reason_documents"):
        result = subprocess.run(["psql", database_url, "-v", "ON_ERROR_STOP=1", "-At", "-c",
                                 "SELECT provider_instance_id||':'||encode(content_hash,'hex')||':'||payload "
                                 f"FROM {table} ORDER BY provider_instance_id,content_hash"],
                                capture_output=True, timeout=30, check=True)
        records.append(result.stdout)
    return tuple(records)


def run(case, binaries, predecessor):
    expected = load(case / "predecessor.json")
    calls, receipts, next_calls = (load(case / name) for name in ("calls.json", "receipts.json", "next.json"))
    POC.require(expected["version"] == 3 and expected["checkpoint"]["engine_version"] == 3,
                "expected the explicit supported V3 predecessor")
    for directory in (binaries, predecessor):
        print("whole-provider SHA-256:", hashlib.sha256((directory / "whole-provider").read_bytes()).hexdigest(), flush=True)
    for authority in ("rust", "shadow", "sandbox"):
        with ExitStack() as resources:
            directory = Path(resources.enter_context(tempfile.TemporaryDirectory(prefix="catalog-rollback-")))
            database_url = POC.temporary_postgres(resources, directory)
            secret_dir = directory / "secrets"
            subprocess.run([str(binaries / "wallet"), "init-dev-secrets", str(secret_dir)],
                           check=True, stdout=subprocess.DEVNULL)
            # The retained SDK fixture uses this public development identity.
            # All other generated secrets remain random, private and disposable.
            (secret_dir / "root-seed.hex").write_text("01" * 32)
            POC.Client.secret_markers = tuple((name, (secret_dir / name).read_bytes().strip()) for name in
                                            ("service-token", "master-key.hex", "root-seed.hex", "bootstrap-admin-password"))
            common = {"INTERNAL_SERVICE_TOKEN_FILE": str(secret_dir / "service-token")}
            provider_environment = {
                **common, "WHOLE_DATABASE_PATH": str(directory / "current.sqlite"),
                "PROVIDER_AUTHORITY": "sandbox", "MOCK_ROOT_ACCOUNT_FILE": str(case / "root-account.hex"),
            }
            wallet_logs = []
            wallet = POC.launch(resources, binaries / "wallet", POC.available_port(), {
                **common, "CUSTODY_DATABASE_URL": f"sqlite://{directory}/custody.sqlite?mode=rwc",
                "CUSTODY_MASTER_KEY_FILE": str(secret_dir / "master-key.hex"),
                "CUSTODY_ROOT_SEED_FILE": str(secret_dir / "root-seed.hex"),
            }, wallet_logs)
            adapter_environment = {
                **common, "ADAPTER_DATABASE_URL": database_url, "CUSTODY_URL": wallet.base,
                "ALLOWED_ORIGINS": ",".join(POC.ORIGINS), "COOKIE_SECURE": "false",
                "BOOTSTRAP_ADMIN_USERNAME": "admin",
                "BOOTSTRAP_ADMIN_PASSWORD_FILE": str(secret_dir / "bootstrap-admin-password"),
                "OPENAPI_PATH": str(ROOT / "contracts/openapi.json"),
            }
            cli(binaries / "whole-provider", ["import", case / "core.json"], provider_environment)
            with ExitStack() as current:
                logs = []
                provider = POC.launch(current, binaries / "whole-provider", POC.available_port(), provider_environment, logs)
                current_adapter = {**adapter_environment, "MOCK_PROVIDER_URL": provider.base}
                cli(binaries / "adapter-api", ["import-creation-metadata", case / "creation.ndjson"], current_adapter)
                cli(binaries / "adapter-api", ["import-proposal-metadata", case / "proposals.ndjson"], current_adapter)
                cli(binaries / "adapter-api", ["import-catalog-metadata", case / "catalog.ndjson"], current_adapter)
                cli(binaries / "adapter-api", ["import-reason-metadata", case / "reasons.ndjson"], current_adapter)
                cli(binaries / "adapter-api", ["backfill-provider-content"], current_adapter)
                exported = directory / "selected-catalog.ndjson"
                cli(binaries / "adapter-api", ["export-catalog-metadata", exported], current_adapter)
                exported_reasons = directory / "selected-reasons.ndjson"
                cli(binaries / "adapter-api", ["export-reason-metadata", exported_reasons], current_adapter)
                metadata_before = proposal_rows(database_url)
                POC.require(all(metadata_before), "paired rehearsal imported no selected context content")
                POC.scan_secret_logs(logs)
            current_core, old_core = directory / "current.json", directory / "rollback.json"
            cli(binaries / "whole-provider", ["export", current_core], provider_environment)
            cli(binaries / "whole-provider", ["downgrade", current_core, exported, exported_reasons, old_core], provider_environment)
            POC.require(load(old_core) == expected, "owner conversion changed expected current V3 state")
            provider_environment = {**provider_environment, "WHOLE_DATABASE_PATH": str(directory / "predecessor.sqlite"),
                                    "PROVIDER_AUTHORITY": authority}
            cli(predecessor / "whole-provider", ["import", old_core], provider_environment)
            token = (secret_dir / "service-token").read_text().strip()
            authorization = {"Authorization": "Bearer " + token}
            for phase in ("first", "reopened"):
                with ExitStack() as serving:
                    logs = []
                    provider = POC.launch(serving, predecessor / "whole-provider", POC.available_port(), provider_environment, logs)
                    adapter_env = {**adapter_environment, "MOCK_PROVIDER_URL": provider.base}
                    cli(predecessor / "adapter-api", ["backfill-provider-content"], adapter_env)
                    adapter = POC.launch(serving, predecessor / "adapter-api", POC.available_port(), adapter_env, logs)
                    adapter.request("GET", "/api/platform")
                    adapter.request("GET", "/api/catalog")
                    POC.require(proposal_rows(database_url) == metadata_before,
                                "predecessor startup altered API-owned context history")
                    snapshot = provider.request("GET", "/internal/snapshot", headers=authorization)
                    saved = expected["checkpoint"]
                    POC.require(snapshot["schemaVersion"] == 3 and snapshot["info"] == saved["info"],
                                "predecessor did not reopen the retained V3 generation")
                    for collection in ("workers", "projects"):
                        wanted = [value for _, value in sorted(saved[collection].items())]
                        POC.require(snapshot[collection] == wanted, "rollback changed current " + collection)
                    for call, receipt in zip(calls, receipts, strict=True):
                        POC.require(provider.request("POST", "/internal/contracts/call", call, headers=authorization) == receipt,
                                    "retained compact receipt changed on predecessor replay")
                    if phase == "reopened":
                        for call in next_calls:
                            result = provider.request("POST", "/internal/contracts/call", call, headers=authorization)
                            POC.require(result["outcome"]["type"] == "Success", "further legacy provider work failed")
                            POC.require(provider.request("POST", "/internal/contracts/call", call, headers=authorization) == result,
                                        "further legacy work was not idempotent")
                        adapter.authenticate("/api/auth/login", {"username": "admin", "password": (secret_dir / "bootstrap-admin-password").read_text().strip()})
                        operation = adapter.command("POST", "/api/projects", {"title": "API after rollback", "description": "Current V2 cohort still works"})
                        project = adapter.request("GET", "/api/projects/" + operation["receipt"]["createdEntityId"])
                        POC.require(project["title"] == "API after rollback" and project["description"] == "Current V2 cohort still works",
                                    "old API did not preserve further frontend creation")
                    POC.scan_secret_logs(logs)
                output = directory / (phase + ".json")
                cli(predecessor / "whole-provider", ["export", output], provider_environment)
                observed = load(output)
                if phase == "first":
                    POC.require(observed == expected, "replay/import changed complete predecessor state")
                else:
                    before, after = expected["checkpoint"], observed["checkpoint"]
                    POC.require(after["balances"] == before["balances"] and after["minted_units"] == before["minted_units"],
                                "further work changed funds")
                    POC.require(after["workers"] == before["workers"] and after["next_task_id"] == before["next_task_id"],
                                "further work changed workers or task counters")
                    POC.require(len(after["projects"]) == len(before["projects"]) + 2, "further Project accounting changed")
                    POC.require(len(after["receipts"]) == len(before["receipts"]) + len(next_calls) + 1
                                and len(after["events"]) == len(before["events"]) + len(next_calls) + 1,
                                "further receipt/event accounting changed")
                    for operation, receipt in before["receipts"].items():
                        POC.require(after["receipts"][operation] == receipt, "retained bytes or receipt changed")
            # The direct legacy Project has no API preimage until owner recovery;
            # the further REST Project already has its atomically staged record.
            recovered = directory / "legacy-creation.ndjson"
            cli(predecessor / "whole-provider", ["recover-creation-metadata", output, recovered], provider_environment)
            with ExitStack() as recovery:
                logs = []
                provider = POC.launch(recovery, predecessor / "whole-provider", POC.available_port(), provider_environment, logs)
                adapter_env = {**adapter_environment, "MOCK_PROVIDER_URL": provider.base}
                cli(predecessor / "adapter-api", ["import-creation-metadata", recovered], adapter_env)
                cli(predecessor / "adapter-api", ["backfill-provider-content"], adapter_env)
                POC.scan_secret_logs(logs)
            POC.scan_secret_logs(wallet_logs)
            POC.require(proposal_rows(database_url) == metadata_before, "old API altered context documents")
            print("PASS", authority, "current-state rollback: successful 54–57, funded state, API/PostgreSQL restart, replay and further REST work", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", required=True, type=Path)
    parser.add_argument("--candidate-binaries", required=True, type=Path)
    parser.add_argument("--predecessor-binaries", required=True, type=Path)
    args = parser.parse_args()
    run(args.case.resolve(), args.candidate_binaries.resolve(), args.predecessor_binaries.resolve())
