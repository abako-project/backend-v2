"""Verify REST-007/008 deployment boundaries without application secrets."""

import json
import os
from pathlib import Path
import subprocess
import sys
import time


INFRA = Path(__file__).resolve().parent
IMAGE = "nginx:1.28.3-alpine"


def run(*args, check=True, env=None):
    return subprocess.run(
        list(args), check=check, text=True,
        capture_output=True, timeout=60, env=env,
    )


def verify_compose():
    # Only harmless synthetic paths are rendered, not the caller's secret setup.
    env = dict(os.environ, LOCAL_UID="1000", LOCAL_GID="1000",
               KUNVENO_LOCAL_DIR="/tmp/kunveno-config-check-not-created",
               EXTERNAL_FRONTEND_IMAGE="team/frontend:verification",
               ALLOWED_ORIGINS="http://localhost:8088")
    args = ["docker", "compose", "-f", str(INFRA / "compose.yaml")]
    config = json.loads(run(*args, "config", "--format", "json", env=env).stdout)
    services = config["services"]
    assert set(services) == {"adapter-api", "wallet", "mock-provider", "frontend", "gateway"}
    assert {name for name, svc in services.items() if svc.get("ports")} == {"gateway"}
    assert services["gateway"]["ports"][0]["host_ip"] == "127.0.0.1"
    assert services["gateway"]["ports"][0]["published"] == "8088"
    assert set(services["gateway"]["networks"]) == {"edge"}
    assert set(services["frontend"]["networks"]) == {"edge"}
    assert set(services["adapter-api"]["networks"]) == {"edge", "backend"}
    assert config["networks"]["backend"]["internal"] is True
    for name, service in services.items():
        assert service["user"].split(":")[0] != "0", name
        assert service["read_only"] is True, name
        assert service["cap_drop"] == ["ALL"], name
        assert "no-new-privileges:true" in service["security_opt"], name
        assert len(service["tmpfs"]) == 1 and "size=" in service["tmpfs"][0], name
        for volume in service.get("volumes", []):
            assert "docker.sock" not in volume["source"], name
        if name in {"wallet", "mock-provider"}:
            assert set(service["networks"]) == {"backend"}, name
    assert not services["frontend"].get("secrets")
    assert not services["gateway"].get("secrets")
    assert {s["source"] for s in services["wallet"]["secrets"]} == {
        "master-key.hex", "root-seed.hex", "service-token"}
    override = json.loads(run(
        *args, "-f", str(INFRA / "compose.external-frontend.yaml"),
        "config", "--format", "json", env=env,
    ).stdout)
    assert "build" not in override["services"]["frontend"]
    assert override["services"]["frontend"]["image"] == "team/frontend:verification"
    for name in {"adapter-api", "wallet", "mock-provider"}:
        assert override["services"][name] == services[name], name
    print("Compose isolation and external frontend override: PASS")


def verify_nginx():
    network = f"kunveno-infra-check-{os.getpid()}"
    containers = []
    run("docker", "network", "create", "--internal", network)
    try:
        for label, config, alias in [
            ("adapter", "test-upstream.conf", "adapter-api"),
            ("frontend", "frontend.conf", "frontend"),
            ("gateway", "gateway.conf", "gateway"),
        ]:
            name = f"{network}-{label}"
            args = [
                "docker", "run", "--detach", "--name", name,
                "--network", network, "--network-alias", alias,
                "--user", "101:101", "--read-only", "--cap-drop", "ALL",
                "--security-opt", "no-new-privileges:true",
                "--tmpfs", "/tmp:rw,noexec,nosuid,size=32m",
                "--mount", f"type=bind,src={INFRA / 'nginx' / config},dst=/etc/nginx/nginx.conf,readonly",
            ]
            if label == "frontend":
                args += ["--mount", f"type=bind,src={INFRA / 'nginx/config.js'},dst=/etc/kunveno/config.js,readonly"]
            result = run(*args, "--entrypoint", "nginx", IMAGE, "-g", "daemon off;", check=False)
            # Docker may create a container even when its startup subsequently fails.
            if result.returncode != 0:
                existing = run("docker", "container", "inspect", name, check=False)
                if existing.returncode == 0:
                    containers.append(name)
                raise RuntimeError(result.stderr)
            containers.append(name)
            run("docker", "exec", name, "nginx", "-t")
        gateway = containers[-1]

        def get(path, *headers):
            args = ["docker", "exec", gateway, "wget", "-S", "-O", "-", "-T", "3"]
            for header in headers:
                args += ["--header", header]
            return run(*args, f"http://127.0.0.1:8080{path}", check=False)

        for _ in range(40):
            response = get("/")
            if response.returncode == 0:
                break
            time.sleep(0.25)
        assert response.returncode == 0, response.stderr
        assert "Welcome to nginx" in response.stdout
        response = get("/api")
        assert "location: /api/" in response.stderr.lower(), response.stderr
        response = get("/config.js")
        assert response.returncode == 0 and 'KUNVENO_API_BASE = "/api"' in response.stdout
        assert "no-store" in response.stderr
        response = get("/api/probe?probe=preserved", "Origin: http://localhost:8088",
                       "X-CSRF-Token: probe-csrf", "Cookie: session=probe-cookie")
        assert response.stdout == "GET|/api/probe|preserved|http://localhost:8088|probe-csrf|session=probe-cookie", response
        response = get("/api/events", "Last-Event-ID: 42")
        assert response.returncode == 0 and "text/event-stream" in response.stderr
        assert response.stdout == "id: 42\nevent: probe\ndata: ok\n\n", response
        for path in ["/internal", "/internal/signing-jobs", "/api/internal/signing-jobs",
                     "/api/%69nternal/signing-jobs", "/health", "/ready", "/api/health"]:
            response = get(path)
            assert "404 Not Found" in response.stderr, (path, response)
        print("Nginx syntax, public routing, credential headers, SSE format/cursor, internal denial: PASS")
    finally:
        for name in reversed(containers):
            run("docker", "container", "rm", "--force", name)
        run("docker", "network", "rm", network)


if __name__ == "__main__":
    try:
        verify_compose()
        if "--config-only" not in sys.argv[1:]:
            verify_nginx()
    except subprocess.CalledProcessError as error:
        sys.exit(error.stderr or str(error))
