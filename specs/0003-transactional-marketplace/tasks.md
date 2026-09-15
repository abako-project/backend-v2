# Implementation graph

Status update 2026-09-15: POC-00 through POC-06 implementation is integrated;
POC-07 independent verification remains open. The similarly named frontend-build
handoff is not POC-07 completion. See `status.md` and
`../../docs/project/porting-coverage.md` for the remaining coverage and parity gaps.

Each writer uses its own local branch/worktree and preserves other writers' changes. Agent shell commands use RTK; human instructions and public scripts do not require it. The root integrator owns shared manifests, lockfile, requirements, and final integration. The six completed implementation worktrees have been removed.

| Task | Writer | Scope | Dependency |
|---|---|---|---|
| POC-00 | root integrator | specs, ADRs, Cargo workspace/dependencies, handoffs | Approved product decisions |
| POC-01 | contracts agent | crates/domain-primitives, crates/generated-contracts | POC-00 |
| POC-02 | mock agent | services/mock-provider | POC-01 frozen contracts |
| POC-03 | custody agent | services/wallet | POC-01 frozen contracts |
| POC-04 | adapter agent | services/adapter-api | POC-01 frozen contracts |
| POC-05 | frontend agent | apps/leptos-web | POC-01 frozen contracts |
| POC-06 | root integrator | infra, integration tests, public OpenAPI/docs, shared-root updates | Work proceeds alongside POC-02 through POC-05 |
| POC-07 | independent verifier | read-only integrated tree and its handoff | POC-02 through POC-06 integrated |

POC-02 through POC-05 can run in parallel. Contract changes are routed back to the integrator, never patched independently by consumers. Verify the integrated tree before completion. Do not create GitHub resources or production deployments.
