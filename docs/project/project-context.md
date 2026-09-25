# Project Context

Updated 2026-09-24 against the backend integration branch.

Kunveno is a Rust port of the core mock-backed marketplace with approved changes.
Legacy: the separate backend repository. Its `main` contains the
provider-owned task-storage change (`d408641`) through merge `3e2b929`.
This is not a complete compatibility port of every legacy package.
See [coverage](porting-coverage.md).

| Dimension | Current implementation |
|---|---|
| Backend | Axum adapter-api, wallet custody, atomic mock-provider |
| Business ownership | Mock modules/contract instances; one transaction per command |
| Frontend | Independent Leptos CSR and external frontend through same REST/SSE API |
| Storage | Adapter PostgreSQL; custody SQLite; mock SQLite or memory |
| Messaging | Durable queues and event polling; no RabbitMQ |
| Authentication | Classic login, cookie sessions, CSRF, backend sr25519 signing; WebAuthn registration and verification integrated, login start awaiting identity lookup policy |
| Deployment | Local Docker Compose and Nginx on localhost:8088 |
| Rust toolchain/MSRV | 1.96.1 |
| Repository | Local Git with a configured origin; publishing requires an explicit request |
| Product license | Undecided; possible future FOSS and commercial use |

Approved behavior: SPEC-0001 through SPEC-0006 and ADR-0001. Calendars use weekly
capacity; matching uses all required skills without comparing roles. Reputation is
separate by mode. Planning is negotiated, execution fully funded into escrow, and
payouts cover each requirement. Execution activates milestones sequentially,
while assignments and reservations are atomic upfront. Adapter PostgreSQL holds
descriptive profiles; the mock holds simulated Bramp requests and balances.
The mock owns skill associations and operator-decided worker requests. Passkey
credentials are adapter-owned, but login start awaits a decision on email versus
username. A current milestone rejection can lead to a
public Open dispute and whole-project freeze; resolution is not implemented.

HSM/envelope encryption and online rotation are not implemented. Custody directly
encrypts seeds using a runtime-file master key. SPEC-0001 now requires a reviewed
rotation and recovery design before real value, not for this local POC.

The owner excluded Leptos and cargo-deny gates from this backend closeout and
described commercial/FOSS-compatible licensing goals. This is not a claim that
dependency policy passes. Independent POC-07 and TASK-005 acceptance is complete
for the local mock-backed POC only.

No real assets, blockchain provider, wallet login, arbitration, worker acceptance or
resignation, Kubernetes or production recovery are delivered by this POC.
