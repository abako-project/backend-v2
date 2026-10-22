# Specification index

Reviewed 2026-10-21. The linked specifications define approved requirements;
approval does not establish verification of every later commit. See
[current implementation and limits](docs/project/project-context.md) for product
status and each feature's recorded evidence for observed results.

| Specification | Approval | Scope |
|---|---|---|
| [0001: custodial signing](specs/0001-custodial-wallet-signing/spec.md) | APPROVED | Encrypted seed storage, principal binding and durable signing jobs |
| [0002: REST and frontend isolation](specs/0002-rest-frontend-isolation/spec.md) | APPROVED | Public REST/SSE and independently deployed browser clients |
| [0003: transactional marketplace](specs/0003-transactional-marketplace/spec.md) | APPROVED | Matching, capacity, proposals, milestones, escrow, compensation and final evaluations, including later approved amendments |
| [0004: Virto compatibility](specs/0004-virto-compatibility/spec.md) | APPROVED | Mock Bramp/catalog and username-first passkey login |
| [0005: dispute opening](specs/0005-dispute-opening/spec.md) | APPROVED | Current-rejection dispute opening and project freeze; resolution excluded |
| [0006: backend parity](specs/0006-backend-parity/spec.md) | APPROVED | Profiles/images, catalogs, task-storage routes, PostgreSQL and project briefs |
| [0007: proposal review](specs/0007-proposal-review-presentation/spec.md) | APPROVED | Presentation metadata and immutable scope-review comments |
| [0008: dispute conversations](specs/0008-dispute-conversations/spec.md) | APPROVED | Repeated public arguments and private case conversation |
| [0009: Digesta rename](specs/0009-digesta-rename/spec.md) | APPROVED | Naming and existing-environment compatibility |
| [0010: preview deployment](specs/0010-backend-preview-deployment/spec.md) | APPROVED | Separate mock-backed server preview |
| [0011: backend dev host](specs/0011-backend-dev-host/spec.md) | APPROVED | Preview API hostname correction to dev.abako.xyz |
| [0012: platform profile](specs/0012-platform-profile/spec.md) | APPROVED | Deployment branding, vocabulary, currency display and compatible custom project types |

| [0013: ink! v5 sandbox](specs/0013-ink-v5-sandbox/spec.md) | APPROVED | Staged canonical contract execution with compatibility, persistence and migration gates |
| [0014: evidence commitments](specs/0014-evidence-commitments/spec.md) | DRAFT | Adapter-owned immutable evidence and fixed-size onchain commitments; anchoring scope pending |

The [repository simplification plan](docs/project/repository-simplification.md)
is approved as SIMPLIFY-001. It preserves application behavior and contracts.
Historical acceptance reports remain under `progress/handoffs/`.
