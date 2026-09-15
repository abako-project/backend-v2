# System Threat Model

The authoritative custody threat register is [SPEC-0001's threat model](../../specs/0001-custodial-wallet-signing/threat-model.md). This page summarizes the current system boundary.

Assets are wallet signing authority, encrypted seeds and master keys, login/session credentials, business state, simulated escrow, operation receipts and notification privacy.

The browser crosses the adapter session/CSRF/CORS boundary. The adapter reaches private bearer-authenticated custody/provider endpoints. Custody signs validated mock calls; the mock verifies signatures and domain permissions before committing state, replay metadata and events atomically.

Current controls include encrypted seeds with associated data, separate runtime secrets, bounded signing jobs, per-wallet ordering, idempotency, session revocation, recipient filtering and private Compose networking. These controls do not make a compromised host or trusted adapter harmless.

The POC accepts host-memory exposure and has no HSM integration, real blockchain execution, production TLS/workload identity, verified backup regime or key-rotation procedure. It must not hold real assets.

TASK-005 remains open: complete diagnostic secret scanning and reconcile the encryption-key-rotation requirement before final acceptance/security closure. Existing tests and handoffs are evidence for their named scenarios, not a completed production assessment.
