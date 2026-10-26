"""Exercise a frozen provider executable; only disposable test files are used."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

binary, database, root, authority, directory = sys.argv[1:]
directory = Path(directory)
token = "calendar-rollback-test-token-00000000"
token_file = directory / "service.token"
token_file.write_text(token)
token_file.chmod(0o600)
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
environment = dict(os.environ, WHOLE_DATABASE_PATH=database,
                   MOCK_ROOT_ACCOUNT_FILE=root, PROVIDER_AUTHORITY=authority,
                   INTERNAL_SERVICE_TOKEN_FILE=str(token_file),
                   BIND_ADDR=f"127.0.0.1:{port}")


def request(path, payload=None):
    body = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=body,
                                 headers={"Authorization": f"Bearer {token}",
                                          "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.load(response)


with (directory / "preceding-server.log").open("w") as log:
    process = subprocess.Popen([binary, "serve"], env=environment, stdout=log, stderr=log)
    try:
        for attempt in range(300):
            if process.poll() is not None:
                raise RuntimeError("frozen provider exited before readiness")
            try:
                request("/internal/info")
                break
            except (urllib.error.URLError, TimeoutError):
                if attempt == 299:
                    raise
                time.sleep(0.1)
        replay, fresh = json.loads((directory / "calls.json").read_text())
        receipts = [request("/internal/contracts/call", call) for call in [replay, fresh, fresh]]
        assert receipts[1] == receipts[2], "fresh operation replay changed its receipt"
        (directory / "receipts.json").write_text(json.dumps(receipts))
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
    if process.returncode != 0:
        raise RuntimeError(f"frozen provider exited with {process.returncode}")
