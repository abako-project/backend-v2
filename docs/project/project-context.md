# Project Context

Updated 2026-09-23.

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
| Storage | Separate SQLite databases; memory feature also available for mock |
| Messaging | Durable queues and event polling; no RabbitMQ |
| Authentication | Classic login, cookie sessions, CSRF, backend sr25519 signing |
| Deployment | Local Docker Compose and Nginx on localhost:8088 |
| Rust toolchain/MSRV | 1.96.1 |
| Repository | Local Git with a configured origin; publishing requires an explicit request |
| Product license | Undecided; possible future FOSS and commercial use |

Approved behavior: SPEC-0001 through SPEC-0003, SPEC-0005 and ADR-0001. Calendars use weekly
capacity; matching uses all required skills without comparing roles. Reputation is
separate by mode. Planning is negotiated, execution fully funded into escrow, and
payouts cover each requirement. A current milestone rejection can lead to a
public Open dispute and whole-project freeze; resolution is not implemented.

HSM/envelope encryption and online rotation were discussed, but are not implemented
or finalized. Custody directly encrypts seeds using a runtime-file master key.
The SPEC-0001 rotation discrepancy remains open.

The owner accepted deferring the two named Leptos maintenance advisories for the POC
and described commercial/FOSS-compatible licensing goals. The cargo-deny policy has
not yet been changed; its last run still failed. Final independent verification is open.

No real assets, blockchain provider, wallet login, arbitration, worker acceptance or
resignation, Kubernetes or production recovery are delivered by this POC.
