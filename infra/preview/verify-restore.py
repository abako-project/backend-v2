#!/usr/bin/env python3
"""Back up the new preview offline and verify a separate stopped restore clone."""

import json
import os
from pathlib import Path
import socket
import subprocess
import time


ROOT = Path("/opt/digesta/preview")
STATE = Path("/srv/digesta/preview")


def run(*args, **kwargs):
    return subprocess.run(args, check=True, timeout=240, **kwargs)


def volume_path(name):
    value = subprocess.check_output(
        ["docker", "volume", "inspect", "--format", "{{.Mountpoint}}", name], text=True, timeout=30)
    path = Path(value.strip())
    if not path.is_absolute() or not path.is_dir():
        raise ValueError("unexpected Docker volume path")
    return path


def verify_restore():
    if os.geteuid() != 0:
        raise ValueError("backup verification requires root on the selected server")
    os.umask(0o077)
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 4081))
    token = time.strftime("%Y%m%d%H%M%S", time.gmtime())
    backup = STATE / "backups" / token
    backup.mkdir(parents=True, mode=0o700)
    active = ["docker", "compose", "--env-file", str(ROOT / "deployment.env"), "-f", str(ROOT / "compose.yaml")]
    run(*active, "stop", "--timeout", "30")
    try:
        statuses = json.loads(subprocess.check_output(["docker", "inspect",
            "digesta-preview-api-1", "digesta-preview-wallet-1",
            "digesta-preview-contract-provider-1", "digesta-preview-postgres-1"], timeout=30))
        if any(container["State"]["Running"] for container in statuses):
            raise ValueError("candidate must be fully stopped for a coherent backup")
        run("tar", "--numeric-owner", "-cpf", str(backup / "state.tar"), "-C", str(STATE), "data", "secrets")
        mount = volume_path("digesta-preview_adapter-postgres-data")
        run("tar", "--numeric-owner", "-cpf", str(backup / "postgres.tar"), "-C", str(mount), ".")
    finally:
        run(*active, "up", "--detach", "--wait", "--wait-timeout", "180")
    print("Coherent offline backup saved:", backup)

    project = "digesta-restore-" + token
    clone_root = STATE / "restore-check" / token
    clone_state = clone_root / "state"
    clone_state.mkdir(parents=True, mode=0o700)
    run("tar", "--same-owner", "-xpf", str(backup / "state.tar"), "-C", str(clone_state))
    original_settings = dict(line.split("=", 1) for line in (ROOT / "deployment.env").read_text().splitlines() if line)
    settings = dict(original_settings, DIGESTA_PROJECT_NAME=project,
                    DIGESTA_STATE_DIR=str(clone_state), ADAPTER_HOST_PORT="4081")
    env_file = clone_root / "deployment.env"
    env_file.write_text("".join(f"{name}={value}\n" for name, value in settings.items()))
    os.chmod(env_file, 0o600)
    restored = ["docker", "compose", "--env-file", str(env_file), "-f", str(ROOT / "compose.yaml")]
    try:
        run(*restored, "create", "postgres")
        mount = volume_path(project + "_adapter-postgres-data")
        if any(mount.iterdir()):
            raise ValueError("restore volume must be new and empty")
        run("tar", "--same-owner", "-xpf", str(backup / "postgres.tar"), "-C", str(mount))
        run(*restored, "up", "--detach", "--wait", "--wait-timeout", "180")
        origin = "https://" + settings["PREVIEW_HOST"]
        run("python3", str(ROOT / "verify-api.py"), "--base-url", "http://127.0.0.1:4081",
            "--origin", origin, "--identity-file", str(clone_state / "secrets/smoke-identity.json"),
            "--internal-http")
        print("PASS: restored identity, encrypted wallet/provider state, operation and SSE history")
    finally:
        run(*restored, "stop", "--timeout", "30")
    result = {"backup": str(backup), "restoreProject": project,
              "restoreEnvironment": str(env_file), "restorePassed": True,
              "retained": "Stopped restore containers, state and volume remain for review; no volumes deleted."}
    (backup / "verification.json").write_text(json.dumps(result, indent=2))
    print("Restore clone stopped; retained separately from active and legacy stores.")


if __name__ == "__main__":
    verify_restore()
