# Implementation graph

Status update 2026-09-24: POC-00 through POC-06 implementation is integrated;
POC-07 independent verification is complete for the local mock-backed POC.
The similarly named frontend-build handoff is not its evidence; see
`../../progress/handoffs/POC-07-verification.md` and `status.md`.

Each writer preserves other writers' changes. The root integrator owns shared manifests, lockfile, requirements, and final integration. The six completed implementation worktrees have been removed.

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
