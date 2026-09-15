# REST-POC foundation progress

Current update 2026-09-15: all task commits are integrated and the six temporary
worktrees have been removed. The real-service E2E was rerun successfully with both
storage backends. It covers one milestone/worker, not the legacy multi-team scenario.
See `docs/project/porting-coverage.md` for known functional omissions and test limits.
The owner accepted deferring the two maintenance advisories; policy configuration is
still pending. HSM/rotation discussion has not produced an implemented design.
The evidence below retains its original execution dates.

- Task: REST-POC-INTEGRATION
- Writer: root integrator
- Branch: `feat/rust-rest-poc`
- Worktree: `.`
- Write scope: approved specs, architecture decisions, shared contracts/workspace, integration and deployment files.
- Source backend: `legacy task-storage worktree`, `d408641`.

## Decisions

REST and independent frontends approved. Custody review corrections recorded: instance-bound signatures, no effects during preparation, exact replay before freshness checks, unknown submission outcomes, and durable event ingestion. Reputation defaults approved in conversation: committed requirement minutes, configurable 50/50 coordinator/client weighting, unrated score 5.

## Implementation

Planning fee negotiation and milestone prices are approved: planning acceptance locks its fee, delivery acceptance pays it independently of execution, and each milestone quotes coordinator and per-requirement amounts. See approved SPEC-0003.

Integrated local commits: shared contracts/OpenAPI `63974f1`, Compose/Nginx `1539792`, mock `2b4eab6`, custody `8712246`, adapter `6315f9b`, and Leptos `7f74f89`. Domain state belongs to the mock; adapter state is limited to authentication, transport operations and notification delivery/read state.

The client and assigned coordinator may cancel or dispute their project. This permission was explicitly confirmed on 2026-09-09 and recorded in SPEC-0003 and its Gherkin scenarios. Public worker listings aggregate weekly commitments without leaking project identifiers.

## Evidence

As of 2026-09-14: workspace check and Clippy pass with Rust 1.96.1; nextest passes 46 tests across 10 binaries, with loopback permission required by HTTP tests. Memory-only mock tests pass 8 cases and SQLite-only tests pass 11. WASM compilation and native builds pass. The frontend image now builds directly with wasm-bindgen-cli 0.2.128 because Trunk 0.21.14's locked build-only graph contained vulnerable crossbeam-channel 0.5.14.

The integrated real-service flow passed for both SQLite and memory: login, signing, escrow, assignment, task authorization, time-weighted scores, deliberately lost submission response, exact retry, and SSE resume/read state. The complete Compose stack built and reached healthy state; its loopback-only gateway served the mounted Leptos UI and OpenAPI while returning 404 for `/internal` and `/health`. `scripts/poc-e2e.py` owns and cleans up its disposable services, keys and databases.

`cargo audit` exits successfully with two informational unmaintained-dependency warnings from Leptos. `cargo deny` remains blocked by those warnings and missing CC0/Boost license allowances. The user requested an explanation and safe dependencies, not approval to ignore the warnings; no exception has been applied. Registry sources and version bans pass after adding explicit versions to local workspace dependencies.

Existing unrelated untracked scaffolding and bytecode were preserved. No remote resources or production deployment were created. This is a local POC, not a production-readiness claim. The unresolved dependency-policy decision is recorded in `docs/dependencies/poc-audit-2026-09-10.md`.
