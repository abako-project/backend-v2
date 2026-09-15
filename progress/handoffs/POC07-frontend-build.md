# POC07 safe frontend build handoff

- Task: replace the vulnerable Trunk build-tool graph without changing the Leptos application.
- Branch: `feat/rust-rest-poc`.
- Worktree: `.`.
- Write scope: frontend container build, static bootstrap document, dependency record and integration evidence.

## Change

`infra/frontend.Dockerfile` installs pinned wasm-bindgen-cli 0.2.128 and runs the existing Leptos binary through `cargo build` plus `wasm-bindgen --target web`. `apps/leptos-web/index.direct.html` loads the generated module and preserves the runtime `/config.js` contract. Trunk is no longer present in the container build.

## Evidence

- The frontend image builds successfully from the committed Cargo.lock.
- The full Compose stack builds and all five services become healthy.
- The loopback-only gateway returns 200 for `/` and `/api/openapi.json`.
- Generated JavaScript and WebAssembly return 200 with `application/javascript` and `application/wasm`.
- Chromium mounts the Leptos login and registration UI.
- Gateway requests to `/internal` and `/health` return 404.
- `infra/verify.py` passes its isolation and Nginx checks.

The disposable Compose project `kunveno-final` and its temporary secrets/data directory were removed after verification. No unrelated containers or workspace files were changed.

## Remaining policy decision

The application lockfile still has the documented CC0-1.0 and Boost Software License dependencies and two unmaintained compile-time Leptos advisories. No cargo-deny exception was added. See `docs/dependencies/poc-audit-2026-09-10.md`.
