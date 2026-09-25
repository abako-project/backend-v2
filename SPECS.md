# Specification Index

Updated 2026-09-25. Requirement approval and verification completion are separate.

| Specification | Approval | Delivery |
|---|---|---|
| [SPEC-0001: custody](specs/0001-custodial-wallet-signing/status.md) | APPROVED | Integrated; historical TASK-005 verified against disposable mock data, not real assets |
| [SPEC-0002: REST/frontend](specs/0002-rest-frontend-isolation/status.md) | APPROVED | Integrated; backend verified against disposable mock data |
| [SPEC-0003: marketplace](specs/0003-transactional-marketplace/status.md) | APPROVED | Integrated; historical POC-07 verified against disposable mock data |
| [SPEC-0004: Virto compatibility](specs/0004-virto-compatibility/status.md) | APPROVED | Mock Bramp/catalog and username-first passkey login implemented in backend branch |
| [SPEC-0005: dispute opening](specs/0005-dispute-opening/status.md) | APPROVED | Implemented and verified on both mock backends: current rejection, public references, project freeze and one response. No timeout, chat or resolution |

The initial marketplace backend was integrated into `master` at `ed6e4f8`; the legacy-scale
E2E and quality refactors followed. Dispute opening is implemented at `a64b747`.
No production release exists. See [port coverage](docs/project/porting-coverage.md)
for legacy differences and test limits. `specs/_templates` contains future-work
forms, not implemented functionality.
