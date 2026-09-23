# SPEC-0004 implementation map

The approved behavior is in `spec.md`; this file records legacy evidence and
work ownership. It is not an alternative contract.

Legacy `main` at `3e2b929` has a passkey ceremony in `packages/virto-api`,
Bramp deposit and withdrawal services in `packages/bramp`, standalone payments
in `packages/virto-api` and `packages/mock-api`, and deploy/query/call wrappers
in `packages/contracts-api`. The mock Bramp endpoints are stubs; deposit
confirmation in the live legacy service tries native transfer and asset mint,
while withdrawal only records Pending. Those implementations are evidence, not
safe authorization or settlement models to copy.

| Task | Primary writer and scope | Depends on | Acceptance |
|---|---|---|---|
| VIR-01 | Passkey: adapter auth and focused tests | PostgreSQL adapter | Verified registration/login maps to existing principal and wallet; replay, wrong origin and UV failures |
| VIR-02 | Bramp: mock domain, signed contracts and focused tests | Shared contract changes | Operator-only deposit confirms once; owner withdrawal holds and cancels once |
| VIR-03 | Integrator: adapter Bramp routes, OpenAPI and events | VIR-02 | Signed `/api` flow and both mock backends |
| VIR-04 | Verifier: E2E and independent security evidence | VIR-01–03 | Full flow, denial, retries and unchanged dispute escrow |

No standalone payments writer, blockchain wrapper, governance service or
frontend task is authorized. Existing typed project/calendar/ratings routes
are mapped in the public API guide; missing actual behavior requires its own
approved change, not an empty wrapper.

Implementation worktrees belong under `/tmp`. Shared Cargo files, signed
contract version, mock dispatch and HTTP router are integrator-owned. Source
changes must preserve SPEC-0005's public case and whole-project freeze.
