#!/usr/bin/env python3
"""Provision fresh preview state on the authorized server; never print secrets."""

import argparse
import os
from pathlib import Path
import pwd
import re
import secrets
import subprocess


POSTGRES = "postgres@sha256:b0f9560a2de083e2cc7382e75f808c7381a32852a7ec49117deedb300e552b24"
EXPECTED = ["master-key.hex", "root-seed.hex", "root-account.hex", "service-token", "bootstrap-admin-password"]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, timeout=600, **kwargs)


def private_write(path, value, uid=0, gid=0):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as output:
        output.write(value)
    os.chown(path, uid, gid)


def provision(host, api_image, wallet_image, provider_image):
    for image in (api_image, wallet_image, provider_image):
        if not re.fullmatch(r"[A-Za-z0-9._/:-]+@sha256:[0-9a-f]{64}", image):
            raise ValueError("backend images must be reviewed immutable digests")
    if os.geteuid() != 0 or host != "dev.abako.xyz":
        raise ValueError("run as root on the approved host for the selected preview origin")
    root = Path("/opt/digesta/preview")
    state = Path("/srv/digesta/preview")
    try:
        account = pwd.getpwnam("digesta")
        if account.pw_dir != "/srv/digesta" or not account.pw_shell.endswith("nologin"):
            raise ValueError("existing digesta account has unexpected ownership settings")
    except KeyError:
        run("useradd", "--system", "--user-group", "--home-dir", "/srv/digesta",
            "--no-create-home", "--shell", "/usr/sbin/nologin", "digesta")
        account = pwd.getpwnam("digesta")
    uid, gid = account.pw_uid, account.pw_gid
    if uid == 0 or gid == 0:
        raise ValueError("application service identity must be non-root")
    for directory in [state, state / "secrets", state / "data", state / "data/custody", state / "data/contracts"]:
        directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(directory, 0o700)
        os.chown(directory, uid, gid)
    settings = (f"DIGESTA_PROJECT_NAME=digesta-preview\nDIGESTA_UID={uid}\nDIGESTA_GID={gid}\n"
                f"DIGESTA_STATE_DIR={state}\nPREVIEW_HOST={host}\n"
                f"API_IMAGE={api_image}\nWALLET_IMAGE={wallet_image}\nCONTRACT_PROVIDER_IMAGE={provider_image}\n"
                f"ALLOWED_ORIGINS=https://{host},https://web.dev.abako.xyz\nADAPTER_HOST_PORT=4080\n")
    env_file = root / "deployment.env"
    if env_file.exists():
        if env_file.read_text() != settings:
            raise ValueError("existing deployment settings differ; refusing an implicit reconfiguration")
    else:
        private_write(env_file, settings)
    secret_dir = state / "secrets"
    existing = [(secret_dir / name).exists() for name in EXPECTED]
    if any(existing) and not all(existing):
        raise ValueError("incomplete secret set; refusing rotation")
    if not any(existing):
        if any(path.is_file() for path in (state / "data").rglob("*")):
            raise ValueError("existing application state has no matching secrets")
        volume = subprocess.run(["docker", "volume", "inspect", "digesta-preview_adapter-postgres-data"],
                                capture_output=True, timeout=30)
        if volume.returncode == 0:
            raise ValueError("existing PostgreSQL volume has no matching secrets")
        run("docker", "pull", wallet_image)
        run("docker", "run", "--rm", "--network", "none", "--user", f"{uid}:{gid}",
            "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
            "--mount", f"type=bind,src={secret_dir},dst=/secrets", wallet_image,
            "init-dev-secrets", "/secrets")
    for name in EXPECTED:
        stat = (secret_dir / name).stat()
        if stat.st_uid != uid or stat.st_mode & 0o077:
            raise ValueError("secret permissions do not match the service account")
    database_password = secret_dir / "postgres-password"
    adapter_env = secret_dir / "adapter.env"
    if database_password.exists() != adapter_env.exists():
        raise ValueError("incomplete database credential set")
    if not database_password.exists():
        run("docker", "pull", POSTGRES)
        postgres_uid = int(subprocess.check_output(
            ["docker", "run", "--rm", "--network", "none", "--entrypoint", "id", POSTGRES, "-u", "postgres"], text=True, timeout=30))
        password = secrets.token_hex(32)
        private_write(database_password, password + "\n", postgres_uid, postgres_uid)
        private_write(adapter_env, "ADAPTER_DATABASE_URL=postgres://digesta:" + password + "@postgres:5432/digesta_adapter\n")
    command = ["docker", "compose", "--env-file", str(env_file), "-f", str(root / "compose.yaml")]
    run(*command, "config", "--quiet")
    run(*command, "pull")
    run(*command, "up", "--detach", "--wait", "--wait-timeout", "180")
    run(*command, "ps")
    print("Preview provisioned with isolated state and private credentials.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--api-image", required=True)
    parser.add_argument("--wallet-image", required=True)
    parser.add_argument("--provider-image", required=True)
    arguments = parser.parse_args()
    provision(arguments.host, arguments.api_image, arguments.wallet_image, arguments.provider_image)
