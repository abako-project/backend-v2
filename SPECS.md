# Specification Index

Updated 2026-09-16. Requirement approval and verification completion are separate.

| Specification | Approval | Delivery |
|---|---|---|
| [SPEC-0001: custody](specs/0001-custodial-wallet-signing/status.md) | APPROVED | Integrated; TASK-005 final acceptance/security closure open |
| [SPEC-0002: REST/frontend](specs/0002-rest-frontend-isolation/status.md) | APPROVED | Integrated; final verification open |
| [SPEC-0003: marketplace](specs/0003-transactional-marketplace/status.md) | APPROVED | Integrated; POC-07 final verification open |
| [SPEC-0004: Virto compatibility](specs/0004-virto-compatibility/plan.md) | DRAFT | Plan only; permissions and ramp settlement rules need approval |
| [SPEC-0005: dispute opening](specs/0005-dispute-opening/status.md) | DRAFT | Rejection, documented opening and public/channel behavior planned; product decisions open |

The marketplace POC was integrated into `master` at `ed6e4f8`; the legacy-scale
E2E, quality refactors and dispute plan were integrated afterward. No production
release exists. See [port coverage](docs/project/porting-coverage.md)
for legacy differences and test limits. `specs/_templates` contains future-work
forms, not implemented functionality.
