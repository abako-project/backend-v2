# System Threat Model

The authoritative custody threat register is [SPEC-0001's threat model](../../specs/0001-custodial-wallet-signing/threat-model.md). This page summarizes the current system boundary.

Assets are wallet signing authority, encrypted seeds and master keys, login/session credentials, business state, simulated escrow, operation receipts and notification privacy.

The browser crosses the adapter session/CSRF/CORS boundary. The adapter reaches private bearer-authenticated custody/provider endpoints. Custody signs validated mock calls; the mock verifies signatures and domain permissions before committing state, replay metadata and events atomically.

Current controls include encrypted seeds with associated data, separate runtime secrets, bounded signing jobs, per-wallet ordering, idempotency, session revocation, recipient filtering and private Compose networking. These controls do not make a compromised host or trusted adapter harmless.

The POC accepts host-memory exposure and has no HSM integration, real blockchain execution, production TLS/workload identity, verified backup regime or key-rotation procedure. It must not hold real assets.

TASK-005 is independently verified for the local mock-backed POC. The E2E runner checks bounded service logs, API responses, SSE and custody metrics for known secret markers; emitted traces are captured in service logs. SPEC-0001 makes HSM-backed rotation a prerequisite for real value rather than POC acceptance. These checks are evidence for their named scenarios, not a completed production assessment.
