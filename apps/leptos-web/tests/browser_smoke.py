#!/usr/bin/env python3
"""Exercise the built CSR app in Chromium against a strict local REST fixture.

Run after building apps/leptos-web/dist: python3 apps/leptos-web/tests/browser_smoke.py
This tests browser behavior, not provider business logic or real custody.
"""
import functools
import http.server
import json
import pathlib
import shutil
import subprocess
import tempfile
import threading


ACCOUNT = "0x" + "03" * 32
ENTITY = "0x" + "04" * 16
INSTANCE = "0x" + "02" * 16
CSRF = "browser-fixture-csrf"
SESSION = {
    "principalId": "0x" + "01" * 16,
    "accountId": ACCOUNT,
    "displayName": "Browser fixture",
    "csrfToken": CSRF,
    "isAdmin": False,
}

DRIVER = r"""
<script>
window.addEventListener('load', async () => {
  const result = document.createElement('output');
  result.id = 'browser-check';
  document.body.appendChild(result);
  const wait = async fn => {
    for (let i = 0; i < 120; i++) {
      const value = fn();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw new Error('Timed out waiting for expected UI state');
  };
  const button = label => [...document.querySelectorAll('button')].find(b => b.textContent === label && !b.disabled);
  const submit = async (label, fields) => {
    const control = await wait(() => button(label));
    const form = control.closest('form');
    for (const [key, value] of Object.entries(fields)) form.elements.namedItem(key).value = value;
    form.requestSubmit();
  };
  try {
    await submit('Iniciar sesión', {username:'fixture', password:'fixture-password-only'});
    await wait(() => document.querySelector('#workers'));
    await wait(() => button('Marcar como leída'));
    const notificationRows = document.querySelectorAll('#notifications li').length;
    if (notificationRows !== 1) throw new Error('SSE duplicate was not deduplicated');
    button('Marcar como leída').click();
    await wait(() => document.querySelector('#notifications').textContent.includes('Leída'));
    await submit('Crear perfil y calendario', {name:'Fixture worker', roles:'3', skills:'1, 2', capacity:'2400'});
    await wait(() => document.querySelector('#operations').textContent.includes('OutcomeUnknown'));
    if (document.querySelector('#operations').textContent.includes('Completada correctamente')) throw new Error('Unknown outcome reported as success');
    await wait(() => document.querySelector('#operations').textContent.includes('Completada correctamente'));
    if (localStorage.length !== 0) throw new Error('Unexpected browser credential persistence');
    button('Cerrar sesión').click();
    await wait(() => button('Iniciar sesión'));
    if (document.querySelector('#notifications')) throw new Error('Private notifications survived logout');
    result.textContent = 'BROWSER_SMOKE_PASS';
  } catch (error) { result.textContent = 'BROWSER_SMOKE_FAIL: ' + error.message; }
});
</script>
"""


class Fixture(http.server.SimpleHTTPRequestHandler):
    state = {"operation": None, "checks": 0, "read": False, "writes": [], "errors": []}

    def log_message(self, *_):
        pass

    def reply(self, status, value=None, cookie=None):
        body = b"" if value is None else json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        if cookie:
            self.send_header("Set-Cookie", cookie)
        self.end_headers()
        self.wfile.write(body)

    def notification(self):
        return {
            "notificationId": 1,
            "readAt": 123 if self.state["read"] else None,
            "event": {
                "providerInstanceId": INSTANCE, "cursor": 1,
                "operationId": ENTITY, "kind": "WorkerRegistered",
                "projectId": None, "entityId": None,
                "recipients": [ACCOUNT], "occurredAt": 120,
            },
        }

    def authorized(self):
        return "fixture_session=active" in self.headers.get("Cookie", "")

    def do_GET(self):
        if not self.path.startswith("/api/"):
            if self.path == "/":
                body = pathlib.Path(self.directory, "index.html").read_bytes() + DRIVER.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            else:
                super().do_GET()
            return
        if not self.authorized():
            self.reply(401, {"code": "unauthenticated", "message": "Sign in"})
            return
        routes = {
            "/api/auth/session": SESSION,
            "/api/catalog": {"roles": [{"id": 3, "name": "backend", "fixed": False}], "skills": [{"id": 1, "name": "rust", "fixed": False}, {"id": 2, "name": "solidity", "fixed": False}], "scorePolicy": {"coordinatorPercent": 50, "clientPercent": 50}},
            "/api/workers": [], "/api/projects": [],
            "/api/balance": {"account": ACCOUNT, "assetId": 1, "available": "1000"},
            "/api/notifications?after=0": {"notifications": [], "nextCursor": 0},
        }
        if self.path in routes:
            self.reply(200, routes[self.path])
        elif self.path.startswith("/api/events?"):
            event = json.dumps(self.notification())
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            self.wfile.write((f"retry: 60000\nid: 1\nevent: notification\ndata: {event}\n\nid: 1\nevent: notification\ndata: {event}\n\n").encode())
        elif self.path == f"/api/operations/{self.state['operation']}":
            self.state["checks"] += 1
            done = self.state["checks"] > 1
            receipt = {"operationId": self.state["operation"], "providerInstanceId": INSTANCE, "origin": ACCOUNT, "nonce": 0, "outcome": {"type": "Success"}, "createdEntityId": ENTITY, "firstEventCursor": 1, "lastEventCursor": 1, "finalizedAt": 120} if done else None
            self.reply(200, {"operationId": self.state["operation"], "status": "Finalized" if done else "OutcomeUnknown", "receipt": receipt, "errorCode": None})
        else:
            self.state["errors"].append(f"Unexpected GET {self.path}")
            self.reply(404, {"code": "not_found", "message": "Unknown fixture path"})

    def do_POST(self):
        try:
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            data = json.loads(body) if body else None
            if self.path == "/api/auth/login":
                assert data == {"username": "fixture", "password": "fixture-password-only"}
                self.reply(200, SESSION, "fixture_session=active; Path=/; HttpOnly; SameSite=Lax")
                return
            assert self.authorized(), "Missing session cookie"
            assert self.headers.get("X-CSRF-Token") == CSRF, "Missing session CSRF token"
            self.state["writes"].append(self.path)
            if self.path == "/api/notifications/1/read":
                assert not self.state["read"], "Read mutation duplicated"
                self.state["read"] = True
                self.reply(200, self.notification())
            elif self.path == "/api/workers":
                assert data == {"displayName": "Fixture worker", "qualifications": {"roleIds": [3], "skillIds": [1, 2]}, "calendar": {"defaultWeeklyMinutes": 2400, "overrides": []}}, "Incorrect typed worker body"
                operation = self.headers.get("Idempotency-Key", "")
                assert operation.startswith("0x") and len(operation) == 34, "Missing typed idempotency key"
                self.state["operation"] = operation
                self.reply(202, {"operationId": operation, "status": "AwaitingSignature"})
            elif self.path == "/api/auth/logout":
                self.reply(204, cookie="fixture_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax")
            else:
                raise AssertionError(f"Unexpected POST {self.path}")
        except (AssertionError, ValueError) as error:
            self.state["errors"].append(str(error))
            self.reply(400, {"code": "fixture_failed", "message": str(error)})


def main():
    dist = pathlib.Path(__file__).resolve().parents[1] / "dist"
    assert (dist / "index.html").is_file(), "Build the frontend assets into apps/leptos-web/dist first"
    chromium = shutil.which("chromium") or shutil.which("chromium-browser")
    assert chromium, "Chromium is required for the browser smoke test"
    handler = functools.partial(Fixture, directory=str(dist))
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        with tempfile.TemporaryDirectory(prefix="kunveno-browser-smoke-") as profile:
            result = subprocess.run([chromium, "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", f"--user-data-dir={profile}", "--dump-dom", "--virtual-time-budget=20000", f"http://127.0.0.1:{server.server_port}/"], capture_output=True, text=True, timeout=60, check=False)
        server.shutdown()
        thread.join(timeout=2)
    assert not Fixture.state["errors"], Fixture.state["errors"]
    assert Fixture.state["writes"] == ["/api/notifications/1/read", "/api/workers", "/api/auth/logout"], Fixture.state["writes"]
    assert '<output id="browser-check">BROWSER_SMOKE_PASS</output>' in result.stdout, result.stdout[-5000:] + result.stderr[-1500:]
    print("PASS: CSR mount, login cookie, CSRF, typed command, idempotency, unknown outcome, polling, SSE dedup/read, logout cleanup")


if __name__ == "__main__":
    main()
