# Local development deployment

This Compose stack implements REST-007/008 and ADR-0001. It is a local POC, not a production deployment. Nginx publishes only `127.0.0.1:8088`: `/api/` goes to the adapter and `/` to an independently built frontend. Custody and the mock have no published ports. There is no Docker socket mount or automatic proxy discovery.

## Start

Run from the workspace root on Linux with Docker Compose 2.24.4 or newer and a running Docker daemon accessible to your user. Host Rust 1.96.1 is used to generate development secrets. Container builds install their own toolchain and wasm-bindgen-cli 0.2.128.

```sh
export LOCAL_UID="$(id -u)"
export LOCAL_GID="$(id -g)"
export KUNVENO_LOCAL_DIR="$(mktemp -d /tmp/kunveno-local.XXXXXX)"
install -d -m 700 "$KUNVENO_LOCAL_DIR/secrets" \
  "$KUNVENO_LOCAL_DIR/data/custody" "$KUNVENO_LOCAL_DIR/data/adapter" \
  "$KUNVENO_LOCAL_DIR/data/mock"
cargo run --locked -p wallet -- init-dev-secrets "$KUNVENO_LOCAL_DIR/secrets"
docker compose -f infra/compose.yaml config --quiet
docker compose -f infra/compose.yaml up --build -d --wait
```

The generator creates fresh mode-0600 `master-key.hex`, `root-seed.hex`, `root-account.hex`, `service-token`, and `bootstrap-admin-password` files without printing their contents. Keep the directory and its environment variables for later commands. Never place these files in this repository or in a Docker build context. Do not run the backend as root: `LOCAL_UID` and `LOCAL_GID` must identify the owner of these private files and database directories. For a container-only host, first build the wallet image, then run its `init-dev-secrets /secrets` command with the same UID/GID and the secrets directory bind-mounted there.

Open <http://localhost:8088>. The initial administrator username is `admin`; its generated password is in the private `bootstrap-admin-password` file. Enter it locally into the login form; do not paste it into logs, issues, or chat. No seed or private wallet key is available to browsers.

The two Docker networks are `edge` (gateway, frontend, adapter) and `backend` (adapter, wallet, mock). The latter is internal. Each backend service sees only its own database directory and required secret files. Root filesystems are read-only, temporary files live in bounded tmpfs mounts, privileges are dropped, and backend processes run with the configured host UID/GID.

SQLite data survives ordinary container restarts. To start a fresh mock environment, choose a **new** private directory and generate a coherent new secret/state set. Do not reset only provider state while retaining adapter operations, and do not delete active data while services run. `docker compose down` stops this stack without deleting its bind-mounted files.

The example uses `/tmp`, which the operating system may clear between host reboots.
Save the value of `KUNVENO_LOCAL_DIR` and export it again, along with your UID/GID, to
resume the same environment. Use a private directory outside the repository on a
persistent filesystem if you need longer-lived local data. Never rerun secret
generation over an existing environment. This deployment uses the current file-backed
custody key, not an HSM or the discussed envelope-encryption/rotation design.

## Another frontend

Both frontends consume the same `/api` REST and authenticated SSE contract. Leptos does not run inside the adapter and has no server-function dependency. Its runtime API URL is supplied by `/config.js`, which defaults to `window.KUNVENO_API_BASE = "/api"`; changing the URL requires no backend rebuild. A different public config can be mounted read-only at `/etc/kunveno/config.js` in the frontend container. Never put secrets in that file.

To replace only the frontend with an existing team image that listens on port 8080:

```sh
export EXTERNAL_FRONTEND_IMAGE=your-team/frontend:local
docker compose -f infra/compose.yaml -f infra/compose.external-frontend.yaml \
  up -d --no-build frontend
```

The replacement inherits the read-only filesystem, non-root user, and temporary-directory contract; build it accordingly, or adapt the override to its documented runtime requirements. The gateway resolves the frontend service through Docker DNS, so recreating that service requires no backend rebuild. The replacement must serve its own SPA/static content on `/` and use the documented API, not custody/provider endpoints.

For a separately hosted development frontend, add its **exact** origin to `ALLOWED_ORIGINS` and recreate the adapter:

```sh
export ALLOWED_ORIGINS=http://localhost:8088,http://127.0.0.1:8088,http://localhost:5173
docker compose -f infra/compose.yaml up -d --no-deps adapter-api
```

Use credentialed browser requests and the session-bound CSRF token. Do not use wildcard credentialed CORS. Prefer the same hostname for local frontend/API URLs; unrelated sites require a separate TLS/cookie policy. HTTP and `COOKIE_SECURE=false` are local-only choices. Nginx does not add permissive CORS headers or alter cookies.

## Build and inspect

```sh
# Backend-only builds do not install browser tooling or compile browser WASM.
docker compose -f infra/compose.yaml build adapter-api wallet mock-provider
docker compose -f infra/compose.yaml build frontend
docker compose -f infra/compose.yaml exec gateway nginx -t
docker compose -f infra/compose.yaml ps
python3 infra/verify.py
```

`verify.py` checks the rendered Compose security boundaries and both actual Nginx configurations, then performs an HTTP routing/SSE-format smoke against a disposable test upstream. It requires Docker and Python 3; it reads no application secrets and starts no application services. Add `--config-only` to check Compose without accessing the Docker daemon. The root integration suite separately verifies signed business calls, authentication and incremental notification replay. Health checks use service-local `/health` endpoints; the gateway blocks `/health`, `/ready`, and `/internal` rather than exposing them publicly. SSE uses HTTP/1.1, no proxy buffering/cache, and a one-hour read timeout; the adapter must send heartbeats within that interval. Access logs omit query strings and request bodies.

The image versions are fixed local-development tags, not release digests. A production release requires digest pinning, current image vulnerability/SBOM evidence, TLS, hardened secret management and the corresponding deployment approval. Backend build/runtime images are both Debian Bookworm (glibc); do not export these binaries into Alpine/musl runtimes.

## Primary references

- [Compose networking](https://docs.docker.com/compose/how-tos/networking/)
- [Compose service configuration](https://docs.docker.com/reference/compose-file/services/)
- [Dockerfile-specific ignore files](https://docs.docker.com/build/building/context/#filename-and-location)
- [Nginx proxy buffering and URI forwarding](https://nginx.org/en/docs/http/ngx_http_proxy_module.html)
- [wasm-bindgen browser deployment](https://wasm-bindgen.github.io/wasm-bindgen/examples/without-a-bundler.html)
