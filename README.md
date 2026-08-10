# Agentic Rust Production Template

A Cargo-first operating template for Rust projects built with coding agents.

It keeps one engineering contract, one role system, and one skill library while supporting Codex, OpenCode, Claude Code, and Qwen Code through thin adapters.

The repository contains no active project `Cargo.toml`. Configure the product first, then let the workspace bootstrap create only the members the product needs.

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
scripts/                  bootstrap, worktree, verification, adapter utilities
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

## First Use

```bash
./scripts/setup-tool-links.sh
python3 scripts/generate-agent-adapters.py
./scripts/verify-template.sh
./scripts/configure-project.sh
./scripts/init-workspace.sh
./scripts/install-dev-tools.sh
```

Initialize Git before creating task worktrees:

```bash
git init
git add .
git commit -m "chore: initialize agentic Rust project"
```

Give `prompts/00-adapt-template.md` to the planner. Review its proposal before project-specific governance changes.

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

## GitHub

```bash
gh auth login
./scripts/pin-github-actions.sh
```

Review generated workflow changes before committing them.

## Production Readiness

This template provides production-oriented controls. A real application still needs verified domain requirements, a threat model, service objectives, data lifecycle rules, deployment ownership, backup restoration tests, and release evidence before it can be called production-ready.
