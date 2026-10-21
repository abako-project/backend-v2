# Repository simplification plan

Status: APPROVED — product owner accepted this plan on 2026-10-21.

Task: SIMPLIFY-001. Baseline: `608c70b` on `master`.

Make the product easy to understand, run and review. Remove the reusable
agent-framework machinery and reduce repeated documentation while preserving
the working frontend integration. The approved scope includes the replacement contributor rules in milestone 2;
existing rules apply until that milestone updates their repository sources.

## Scope and compatibility

Keep the three backend services, independent frontends, database ownership,
REST/JSON and SSE contracts, signed provider formats, business rules and seeded
development flows. Keep current image names, service entry points, environment
variables, persistence paths and release-tag conventions.

No service merge, database migration, dependency upgrade, contract migration or
embedded WASM runtime belongs to this cleanup. ink! v5 remains the intended
contract target; executing those contracts in the mock is future work.

Each milestone is a separate PR that can merge and remain usable on its own.
An image tag is optional and requires a separate release instruction. No
deployment is part of the plan. Revert a cleanup PR to undo it; no data rollback
should be necessary.

## Milestone 1: Explain the working product

Estimate: 0.5–1 engineering day. Documentation only.

- Rewrite `README.md` around the product, current capabilities, service diagram,
  prerequisites, one recommended local startup path and verification links.
  Explain the mock, its persistence options and the current gap to Kreivo.
- Use `docs/project/project-context.md` for the current implementation status
  and known limitations. Keep `SPECS.md` as the requirements index and update
  its missing entries. Historical reports remain evidence of past checks.
- Correct conflicting PostgreSQL, task-storage and workspace descriptions in
  the architecture, local development and workspace documentation. Check
  business-rule corrections against approved specifications and tests.
- Link detailed API, business-rule, deployment and disposable-account guidance
  from the README instead of repeating it there. Keep valid recovery guidance
  readily accessible.

Done when a reader can find what works, what is simulated, how to start it and
how to test it from the README. Changed links and documented commands have been
checked. Documentation agrees with service manifests and approved behavior.

## Milestone 2: Remove the agent framework and shorten the process

Estimate: 0.5–1 engineering day. Depends on milestone 1.

Approve the replacement contributor rules before removing their current
sources. Put the human workflow in a short `CONTRIBUTING.md`; keep `AGENTS.md`
as concise repository-specific guidance pointing to that workflow.

Proposed rules:

- Routine documentation, fixes and compatible additions use an issue or PR
  description covering the problem, intended behavior and relevant checks.
- A substantive feature uses one approved feature note with acceptance
  criteria. Add a separate design, threat model or migration plan when needed.
  Retire the mandatory seven-file specification bundle for future work.
- Preserve existing approved requirements, accepted ADRs, public contracts and
  approval boundaries for breaking changes, authentication, custody, destructive
  operations, sensitive dependencies, releases and production deployment.
- Use ordinary feature branches. Require separate worktrees when concurrent
  work needs isolation. Record verification in the PR; create a separate handoff
  only when work actually changes hands or needs a persistent blocker record.

After those rules are approved, remove the following as one coherent change:

| Remove from the product repository | Related cleanup |
|---|---|
| `agents/`, `.agents/skills/`, generated agent adapters and skill symlinks under `.codex/`, `.claude/`, `.qwen/`, `.opencode/` | Remove tracked project model/agent settings; retain a minimal tool entry point only if required to read `AGENTS.md`. Leave personal, ignored settings alone. |
| `models/`, `prompts/`, `templates/`, `opencode.json`, `TEMPLATE-VERSION` | Remove template bootstrap and provider-selection instructions. Preserve required third-party attribution. |
| Agent adapter generators, tool-link setup, model selection/configuration and template verification scripts | Remove their callers and references together. |
| `scripts/new-spec.sh`, `scripts/new-worktree.sh`, template-based `scripts/pin-github-actions.sh` | Document ordinary Git usage and direct review of pinned workflow updates. |
| Generic agent portability, operating-model and model-provider documents | Retain useful project-specific security and writing rules in the contributor guidance. |

Simplify the PR and issue templates to outcome, compatibility impact and test
evidence. Remove placeholder CODEOWNERS entries rather than inventing owners.
Retire redundant `specs/_templates` forms. Keep existing approved specs and
accepted ADRs; mark historical handoffs/research clearly and repair navigation
instead of mechanically moving every old document.

Reusable personal tooling can later be recovered from Git into a separate
tooling repository. Creating and maintaining that repository is not a prerequisite
for this cleanup.

Done when a fresh clone requires no agent setup to build, test or contribute;
active documentation and scripts have no broken references to removed tooling;
and application builds, service E2E tests and container builds still pass. The
current Docker build and release workflow have no direct dependency on the agent
framework, but the removal still needs executable verification.

## Milestone 3: Make local verification and CI agree

Estimate: 1–2 engineering days. Depends on milestone 2.

- Pin the Rust toolchain consistently with the current Docker and release
  compiler. Keep existing dependencies and the lockfile unchanged.
- Make `scripts/verify.sh` the documented entry point for application checks.
  Have local development and CI call the same checks. Separate clearly named
  application, container and dependency-policy checks where prerequisites differ.
- Preserve formatting, strict Clippy, workspace tests, doctests, mock feature
  combinations and the existing real-service E2E in both memory and SQLite modes.
  Supply disposable PostgreSQL explicitly so database tests actually execute.
- Run application verification on PRs and pushes to `master`. Retain tag-only
  image publication, the check that tagged commits belong to `master`, and
  existing image naming. Share verification steps with the release workflow.
- Simplify `scripts/install-dev-tools.sh`: install the tools needed for the
  documented checks; make nightly/Miri, coverage and other specialist tools
  optional. Preserve the independent frontend build check where applicable.
- Keep dependency findings visible with commands and observed results. Document
  the existing deny/audit policy failures separately from application checks;
  do not turn failures into successful checks or broaden exceptions. Resolving
  findings that need dependency or license decisions is separate work.
- Replace the placeholder repository URL in `Cargo.toml`. Record the mismatch
  between package license metadata and the template license notice for an owner
  decision; do not select or change the project license during cleanup.

Done when a documented fresh checkout can start the stack and run verification
without agent tooling, the same application checks pass in PR CI, E2E runs in
both mock modes, and all three backend images build. Reports distinguish checks
that passed, failed or could not run. Dependency-policy findings remain explicit.

## Milestone 4: Review the repository as a grant reviewer

Estimate: 0.5–1 engineering day. Depends on milestone 3.

- Walk through a clean checkout using only the README and its linked guides.
- Follow the existing demonstrated lifecycle through the public API: platform
  profile, login, project/proposal flow, assignment, milestone delivery and
  payment, notifications and the existing dispute-opening behavior.
- Verify the current frontend integration through the gateway, including login
  and SSE. Record browser coverage separately from API-only evidence.
- Check that examples, OpenAPI, deployment instructions, current status and
  roadmap agree. Label future contract-runtime work as proposed.
- Record the tested commit, commands, results and remaining limitations in the
  PR. Fix remaining duplicated guidance and broken links found in the walkthrough.

Done when another developer can understand the service boundaries and mock
limitations, run the documented demo and locate reproducible verification
evidence. No frontend change or data migration is needed to use the result.

## Estimate and limits

Allow roughly 3–5 engineering days, plus review and CI execution time. The
estimate assumes working PostgreSQL/container infrastructure and access to the
current frontend. Allow another day if the clean-checkout exercise reveals
environment problems. Do not absorb unrelated product fixes into cleanup PRs.

Removing or extracting `crates/bloque-rs` or `apps/leptos-web`, combining backend
services and introducing the v5 contract runtime require separate decisions.
The Bloque crate currently appears as a standalone workspace member rather than
a service dependency; that alone does not establish that it should be deleted.

The plan succeeds through fewer prerequisites, consistent instructions and
reproducible evidence. File count is an observation, not a release criterion.

## Execution record — 2026-10-21

| Milestone | Review | Status |
|---|---|---|
| 1: Product documentation | [PR 9](https://github.com/abako-project/backend-v2/pull/9), `81fbd65` | Prepared; documentation and Compose-rendering checks pass |
| 2: Framework removal | [PR 10](https://github.com/abako-project/backend-v2/pull/10), `f92e23d` | Prepared; 154 tracked framework/template files removed; application sources and contracts preserved |
| 3: Verification alignment | [PR 11](https://github.com/abako-project/backend-v2/pull/11), `4a5c46f` | Prepared; shared local/CI gate, both mock feature suites, WASM check and three image builds verified; all eight real-service E2E cases passed; PR CI still running |
| 4: Reviewer walkthrough | Final documentation corrections | Partial; documentation audit complete, Docker gateway/browser acceptance pending |

The reviews are stacked in milestone order and remain drafts. Nothing has been
merged, tagged or deployed. The resulting checkout contains 325 tracked files,
compared with 475 at the baseline; file count is not an acceptance gate.

Verification of `4a5c46f`: formatting, workspace check, strict Clippy,
104 workspace tests against disposable PostgreSQL, two doctests, memory-only and
SQLite-only mock suites, workspace build and frontend WASM check passed. All
three unchanged backend Dockerfile targets built with rootless Podman. Workflow
validation with actionlint, shell/Python syntax, active Markdown file links and
Compose isolation/external-frontend rendering also passed. The complete
`scripts/verify.sh app` command exited successfully, including all eight real-service
E2E cases (four on SQLite, four on memory). GitHub PR verification is still running
at the time of this record; check PR 11 before marking ready.

Persistent acceptance blocker: no Docker daemon is available here. Podman can
build the images, but the Docker-specific Nginx smoke returns 502 with the
configured Docker DNS resolver. The external frontend/browser walkthrough was
not run. Complete gateway/login/SSE browser acceptance on a Docker-capable host
using the README before closing milestone 4. No networking change or production
operation is proposed to bypass this check.

The fresh dependency-policy check fails with six vulnerability advisories in
locked TLS dependencies, plus license-policy and maintenance findings. Bans and
sources pass. `cargo-audit` is unavailable. Current advisory IDs and the separate
license decision are recorded in [supply-chain status](../security/supply-chain.md).
No dependency update, policy exception or project license choice is included.
