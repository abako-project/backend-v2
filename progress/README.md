# Persistent Agent Evidence

Current status is in specification `status.md` files and [SPECS.md](../SPECS.md).
[REST-POC-foundation](handoffs/REST-POC-foundation.md) consolidates integration evidence.

Component handoffs are historical records. Their six worktrees were removed after
integration. Old historical commands describe what agents executed, not current human
requirements. Use [infra/README.md](../infra/README.md) for deployment.

The historical [dispute plan](handoffs/DISPUTE-PLAN.md) and quality handoffs
([test layout](handoffs/QUALITY-TEST-LAYOUT.md),
[mock domain](handoffs/QUALITY-MOCK-DOMAIN.md),
[mock integration tests](handoffs/QUALITY-MOCK-INTEGRATION-TESTS.md)) document
the 2026-09-16 review. The [dispute reanalysis](handoffs/DSP-00-reanalysis.md)
records earlier scope decisions; [DSP-07](handoffs/DSP-07.md) records the
implemented SPEC-0005 flow and verified mock E2E. The
[port review](../docs/project/porting-coverage.md) records current gaps and
unresolved product decisions.

`POC07-frontend-build.md` documents a tooling correction, not the independent POC-07
task. Independent [POC-07](handoffs/POC-07-verification.md) and
[TASK-005](handoffs/SPEC-0001-verification.md) reports now close local mock-backed
POC acceptance only. They do not approve real assets or production. Never store
secrets in reports.
