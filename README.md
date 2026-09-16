# Kunveno Rust POC

Local Rust implementation of Kunveno's marketplace: REST adapter, full custodial signing, transactional mock provider, and independent Leptos frontend. Mock funds, contracts and keys are disposable development data, never production assets.

## Run locally

Use Rust 1.96.1. The [Compose guide](infra/README.md) creates private runtime secrets and starts the services behind Nginx at `http://localhost:8088`. Users and agents run ordinary Cargo, Python and Docker commands. No production deployment exists.

For a disposable end-to-end test without containers:

```sh
cargo build --workspace --all-features --locked
python3 scripts/poc-e2e.py
```

The test starts real adapter, custody, and provider processes with fresh secrets. It runs both mock storage backends and two isolated scenarios: the original security/SSE flow and four milestones with teams of 5/3/4/2 workers. It checks signed operations, planning fees, escrow, assignments, payouts and scores, then removes its temporary data. This is a backend E2E, not a browser-driven full-stack test. See the [porting and test coverage review](docs/project/porting-coverage.md).

## Applications and contracts

| Project | Responsibility |
|---|---|
| `services/adapter-api` | Public REST, login, sessions, operation queue, notifications and SSE |
| `services/wallet` | Encrypted custodial seeds and durable sr25519 signing jobs; no submission |
| `services/mock-provider` | Authoritative business state, signed execution, calendars, escrow, scores, receipts and events |
| `apps/leptos-web` | Independent browser application using the public REST API |
| `crates/domain-primitives` | Validated IDs, integer quantities and week ranges |
| `crates/generated-contracts` | Wire DTOs and immutable signable call construction |

Contract instances are domain objects inside one atomic mock runtime, not one process per worker or proposal. SQLite is the default backend. Cargo features `storage-memory` and `storage-sqlite` may compile together; `MOCK_STORAGE` selects the backend. `mock-seed` initializes missing catalog entries and enables authenticated development funding.

Both frontends consume `/api`, without GraphQL or Leptos server functions. Start with the [HTTP contract](crates/generated-contracts/API.md) and [OpenAPI document](contracts/openapi.json), also served at `/api/openapi.json`. The [deployment guide](infra/README.md#another-frontend) covers the team's separate frontend.

Authenticate with the HttpOnly cookie and send `X-CSRF-Token` on authenticated mutations. Business mutations return `202 OperationRef`, not business success: poll the operation and inspect its receipt's execution outcome. Supply an `Idempotency-Key` for retry safety. SSE cursors do not mark notifications read. Custody and provider endpoints remain internal.

## Verification

```sh
bash scripts/verify.sh
python3 infra/verify.py
python3 scripts/poc-e2e.py
```

These cover workspace gates, mock feature variants, WASM compilation, dependency policy, Nginx isolation and the real signed flow. Actual results and remaining work belong in [progress/handoffs](progress/handoffs/); listing commands is not a claim that every gate has passed.

## Port status

The transactional marketplace, custodial signing, mock storage backends, REST/SSE
adapter and independent frontend are implemented. Each proposal milestone owns one
task storage created with the draft; draft edits preserve that storage when the
milestone key is preserved. The E2E covers four milestones with teams of 5/3/4/2.

This is not complete legacy compatibility. Auxiliary Virto membership/governance,
Bramp, rich profiles and several legacy wrapper APIs remain unimplemented or require
a product decision. Dispute opening is only a reason plus frozen funds today; the
formal dispute expediente described in `Disputas.md` has a draft
[implementation plan](specs/0005-dispute-opening/tasks.md), not an approved or
implemented feature. See the [current port review](docs/project/porting-coverage.md).

## Core Layout

```text
AGENTS.md                 shared project contract
agents/                   canonical agent roles and registry
.agents/skills/           canonical reusable skills
.codex/agents/            generated Codex adapters
.opencode/agents/         generated OpenCode adapters
.claude/agents/           generated Claude Code adapters
.claude/skills            symlink to .agents/skills
.qwen/agents/             generated Qwen Code adapters
.qwen/skills              symlink to .agents/skills
specs/                    approved behavior and Gherkin acceptance criteria
docs/                     architecture, operations, security, research
models/                   dated model profiles
scripts/                  worktree, verification, adapter utilities
```

## Principles

- Specifications decide behavior before implementation.
- Gherkin expresses acceptance behavior when scenarios add value.
- One task has one primary writer, branch, worktree, and write scope.
- Roles are canonical; tool adapters are generated.
- Skills are shared, not copied.
- Model choice is separate from role choice.
- Cargo stays visible. No `justfile` or Makefile is required.
- Service boundaries follow bounded contexts, not tables.
- Verification evidence decides completion.
- Documentation follows simplicity, brevity, clarity, and humanity.

## Agent tooling

The workspace is already configured; no bootstrap step is required. Shared engineering rules live in `AGENTS.md`; reusable skills live in `.agents/skills/`.

Create the first specification:

```bash
./scripts/new-spec.sh "Account registration"
```

Implementation starts only after the authorized owner changes the feature state to `APPROVED`.

## Harnesses

**Codex:** open it in the project root. It reads `AGENTS.md`; generated agents live in `.codex/agents/`.

**OpenCode:** connect a provider with `/connect`. OpenCode discovers `.agents/skills/` directly. Set a verified profile with `./scripts/select-opencode-model.sh <alias>` when desired.

**Claude Code:** `CLAUDE.md` imports `AGENTS.md`. Claude adapters live in `.claude/agents/`; `.claude/skills` points to the canonical skill tree.

**Qwen Code:** Qwen reads `AGENTS.md`. Qwen adapters live in `.qwen/agents/`; `.qwen/skills` points to the canonical skill tree. Use `/auth` for Alibaba Cloud Coding Plan, or run `python3 scripts/configure-qwen-models.py` to register the verified multi-provider catalog. Use `/model` to switch.

## Verified Model Snapshot

```bash
./scripts/show-models.sh
```

The snapshot is dated. Verify provider availability before treating any model ID as permanent. See `docs/agentic/model-provider-setup.md` for provider setup.

## Delivery

`master` is published to the configured origin. Do not create issues, pull requests,
releases or production deployments without a separate request.

## Production Readiness

This is not production custody or a blockchain implementation. Real deployment still requires reviewed chain-specific payload validation, TLS/workload identity, managed key storage, recovery procedures and release approval. Replacing the mock requires a blockchain provider implementation, not merely relabeling its endpoint.
