# SIMPLIFY-001 milestone 1

Approved scope: `docs/project/repository-simplification.md`, milestone 1.
Branch: `simplify/01-product-docs`. Worktree: `/tmp/backend-simplify-01`.
Primary writer: root. Base: `608c70b`.

Changed README, specification index, current project context, project navigation,
system/workspace descriptions, local development and happy-path/porting guidance.
Preserved disposable account instructions in the local development guide.
Recorded owner approval of the simplification plan. No runtime, API, database,
dependency or deployment change.

Verification: changed Markdown file links resolve; shell examples pass `bash -n`;
workspace table matches all seven manifest members; `git diff --check` passes;
`python3 infra/verify.py --config-only` passes; E2E and seeding CLI help pass.
Business-rule corrections checked against approved SPEC-0003, SPEC-0007 and
SPEC-0008. No business test rerun for this documentation-only milestone.

Docker's socket is unavailable. Rootless Podman responds outside the sandbox;
container execution and builds remain to be verified in later milestones.
No image publication, deployment or remote PR has been performed.
