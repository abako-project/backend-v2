# Kunveno Rust backend

Kunveno is a product under development. This Rust workspace provides its REST adapter, custodial signing, a transactional mock provider for development and an independent Leptos test frontend. Mock funds, contracts and keys are disposable test data, never real assets.

## Current delivery dates and final evaluations

The desktop frontend preserves the legacy **global** proposal calendar. A milestone's
“Specific date” selector does not collect/persist an independent milestone date or
bind it to the global date. The mock owns the global preference/date and separate
operational ISO-week windows. Individual milestone dates, their editing and their
scheduling effect are **pending a product decision**; do not infer them from the label.

After completion, 1–5-star evaluations allow only client → coordinator,
coordinator → client and assigned workers, and worker → coordinator. The provider
rejects other targets, duplicate evaluations and self-votes. Reviews do not move funds.
Worker reputation uses only the coordinator evaluation. Coordinator reputation includes
the client and each worker equally by default, weighted by project coordination minutes.
`PUT /api/admin/score-policy` exposes `coordinatorClientVoteWeight` and
`coordinatorWorkerVoteWeight`: relative integers 0–100, default 1/1, not both zero.
Changing these parameters affects future votes only. Counting the **team average as
one vote** alongside the client is documented pending work, not an active mode.
See [approved scoring rules](specs/0003-transactional-marketplace/spec.md#milestones-tracking-scores-and-funds)
and the [happy path](docs/project/happy-path.md).

## Run locally

Use Rust 1.96.1. The [Compose guide](infra/README.md) creates private runtime secrets and starts the services behind Nginx at `http://localhost:8088`. Users and agents run ordinary Cargo, Python and Docker commands. No production deployment exists.

For a disposable end-to-end test without containers, install PostgreSQL's
`initdb`, `pg_ctl` and `psql` commands first:

```sh
cargo build --workspace --all-features --locked
python3 scripts/poc-e2e.py
```

The test starts real adapter, custody, provider and disposable PostgreSQL
processes with fresh secrets. On both mock storage backends it runs four
isolated scenarios: the security/SSE flow, four sequential milestones with
teams of 5/3/4/2 workers, public dispute opening after a rejected
delivery, and auxiliary profile/Bramp/catalog/passkey-boundary checks. It checks
signed operations, escrow, assignments, payouts and scores, then removes its
temporary data. This is a backend E2E, not a browser test. See the
[happy-path guide](docs/project/happy-path.md) and
[porting coverage](docs/project/porting-coverage.md). Passing historical E2E
records do not verify the latest integration commit; check its handoff.

## Applications and contracts

| Project | Responsibility |
|---|---|
| `services/adapter-api` | Public REST, login, PostgreSQL profiles/sessions/operation queue, notifications and SSE |
| `services/wallet` | Encrypted custodial seeds and durable sr25519 signing jobs; no submission |
| `services/mock-provider` | Authoritative business state, signed execution, calendars, escrow, scores, receipts and events |
| `apps/leptos-web` | Independent browser application using the public REST API |
| `crates/domain-primitives` | Validated IDs, integer quantities and week ranges |
| `crates/generated-contracts` | Wire DTOs and immutable signable call construction |

Contract instances are domain objects inside one atomic mock runtime, not one
process per worker or proposal. SQLite is the default *mock* backend. Cargo
features `storage-memory` and `storage-sqlite` may compile together;
`MOCK_STORAGE` selects the backend. Adapter-owned data uses PostgreSQL.
`mock-seed` initializes missing catalog entries. `POST /api/admin/fund` is
available only with `ENABLE_MOCK_FUNDING=true` for fixtures; normal local
funding uses simulated Bramp requests and operator confirmation.

Both frontends consume `/api` as REST/JSON and SSE clients. Start with the [happy-path guide](docs/project/happy-path.md), [HTTP contract](crates/generated-contracts/API.md) and [OpenAPI document](contracts/openapi.json), also served at `/api/openapi.json`. The [deployment guide](infra/README.md#another-frontend) covers the team's separate frontend.

Authenticate with the HttpOnly cookie and send `X-CSRF-Token` on authenticated mutations. Business mutations return `202 OperationRef`, not business success: poll the operation and inspect its receipt's execution outcome. Supply an `Idempotency-Key` for retry safety. SSE cursors do not mark notifications read. Custody and provider endpoints remain internal.

## Verification

```sh
cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked
python3 scripts/poc-e2e.py
```

This exercises the real signed backend flow with disposable PostgreSQL and both mock storage modes. Frontend, dependency-policy and Nginx checks are separate; command listings are not passing-result claims. See [porting coverage](docs/project/porting-coverage.md).

## Port status

The transactional marketplace, custodial signing, mock storage backends and
REST/SSE adapter are implemented. Each proposal milestone owns a task storage
created with its draft; at least one task per storage is required before
submission. Execution approval assigns and reserves every milestone atomically,
activates only the first, and later accepts each in sequence until the project
is `Completed`. The expanded E2E fixture covers four milestones with teams of
5/3/4/2. Adapter descriptive profiles and their public PNG/JPEG/WebP images
use PostgreSQL. The mock models
Bramp deposits, withdrawal holds and operator-owned skill requests.

This is not complete legacy compatibility. Passkey registration and login use
the existing username and custodial account; email verification is not required
for passkey access. Password login remains available. Virto governance,
membership, real Kreivo calls, banking and generic payments are outside this
phase. The formal public dispute case described in `Disputas.md` is implemented
for the approved current scope and consolidated in
[SPEC-0005](specs/0005-dispute-opening/spec.md): current rejection, public
written opening arguments, project-wide freeze and public case presentation.
[SPEC-0008](specs/0008-dispute-conversations/spec.md) adds immutable repeated
public arguments and a private case conversation in adapter/PostgreSQL. Public
history includes later arguments but never private messages. By default the client
and coordinator can both read and write in the channel. Set
`DISPUTE_CHANNEL_ALLOW_PARTICIPANTS=true` in the Compose launch environment to
permit client, coordinator and assigned project workers to read/write that channel;
public arguments remain client/coordinator only. The API returns `canWrite`.
Timeout and formal dispute resolution/settlement remain excluded. Normal final
milestone acceptance completes the project and settles funds independently;
final evaluations do not move funds. See the [current port review](docs/project/porting-coverage.md).

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

## Local development accounts

These disposable accounts are for the local mock backend only. Each starts with
1000 simulated KVN. Workers have 40 hours/week, all seeded skills and all
non-coordinator roles so the complete development flow can be exercised.
Coordinators additionally have server-granted eligibility and Coordinator mode.

| View | Username | Display name | Password |
| --- | --- | --- | --- |
| Worker | `worker1` | Worker 1 | `Worker 1 1234!` |
| Worker | `worker2` | Worker 2 | `Worker 2 1234!` |
| Worker | `worker3` | Worker 3 | `Worker 3 1234!` |
| Worker | `worker4` | Worker 4 | `Worker 4 1234!` |
| Worker | `worker5` | Worker 5 | `Worker 5 1234!` |
| Worker | `worker6` | Worker 6 | `Worker 6 1234!` |
| Worker | `worker7` | Worker 7 | `Worker 7 1234!` |
| Worker | `worker8` | Worker 8 | `Worker 8 1234!` |
| Worker | `worker9` | Worker 9 | `Worker 9 1234!` |
| Worker | `worker10` | Worker 10 | `Worker 10 1234!` |
| Coordinator | `coordinator1` | Coordinator 1 | `Coordinator 1 1234!` |
| Coordinator | `coordinator2` | Coordinator 2 | `Coordinator 2 1234!` |
| Coordinator | `coordinator3` | Coordinator 3 | `Coordinator 3 1234!` |
| Client | `client1` | Client 1 | `Client 1 1234!` |
| Client | `client2` | Client 2 | `Client 2 1234!` |
| Client | `client3` | Client 3 | `Client 3 1234!` |

Create/check these accounts through the API after starting the local stack:

```sh
python3 scripts/seed-dev-users.py --base-url http://localhost:8088 \
  --admin-password-file "$KUNVENO_LOCAL_DIR/secrets/bootstrap-admin-password"
```

The script reuses existing actors, verifies their profiles and coordinator modes,
and tops up balances below 1000 KVN through simulated Bramp deposits. It never
resets the databases. Development passwords above are not production credentials.
