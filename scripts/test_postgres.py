#!/usr/bin/env python3
"""Run a test command with disposable PostgreSQL unless a test URL is supplied."""

from contextlib import ExitStack
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile


def available_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def temporary_postgres(stack, directory):
    data = directory / "postgres"
    socket_dir = directory / "pg-socket"
    socket_dir.mkdir()
    port = available_port()
    subprocess.run(["initdb", "-D", str(data), "-U", "digesta", "-A", "trust",
                    "--no-instructions"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["pg_ctl", "-D", str(data), "-o",
                    f"-F -c listen_addresses=127.0.0.1 -p {port} -k {socket_dir}",
                    "-w", "start"], check=True, stdout=subprocess.DEVNULL)
    stack.callback(subprocess.run, ["pg_ctl", "-D", str(data), "-m", "immediate",
                                   "-w", "stop"], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["psql", "-h", "127.0.0.1", "-p", str(port), "-U", "digesta",
                    "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-c",
                    "CREATE DATABASE digesta"], check=True, stdout=subprocess.DEVNULL)
    return f"postgres://digesta@127.0.0.1:{port}/digesta"


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 scripts/test_postgres.py <command> [arguments...]", file=sys.stderr)
        return 2
    with ExitStack() as stack:
        env = dict(os.environ)
        if not env.get("TEST_ADAPTER_DATABASE_URL"):
            directory = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="digesta-tests-")))
            env["TEST_ADAPTER_DATABASE_URL"] = temporary_postgres(stack, directory)
        return subprocess.run(sys.argv[1:], env=env, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
