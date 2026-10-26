# Requirements

These approved specifications define product behavior. [Current status](../docs/status.md)
and [verification](../ci/README.md) distinguish implementation from release readiness.

| Specification | Approval | Scope |
|---|---|---|
| [0001: custodial signing](0001-custodial-wallet-signing/spec.md) | APPROVED | Encrypted seed storage, principal binding and durable signing jobs |
| [0002: REST and frontend isolation](0002-rest-frontend-isolation/spec.md) | APPROVED | Public REST/SSE and independently deployed browser clients |
| [0003: transactional marketplace](0003-transactional-marketplace/spec.md) | APPROVED | Matching, capacity, proposals, milestones, escrow, compensation and final evaluations, including later approved amendments |
| [0004: Virto compatibility](0004-virto-compatibility/spec.md) | APPROVED | Mock Bramp/catalog and username-first passkey login |
| [0005: dispute opening](0005-dispute-opening/spec.md) | APPROVED | Current-rejection dispute opening and project freeze; resolution excluded |
| [0006: backend parity](0006-backend-parity/spec.md) | APPROVED | Profiles/images, catalogs, task-storage routes, PostgreSQL and project briefs |
| [0007: proposal review](0007-proposal-review-presentation/spec.md) | APPROVED | Presentation metadata and immutable scope-review comments |
| [0008: dispute conversations](0008-dispute-conversations/spec.md) | APPROVED | Repeated public arguments and private case conversation |
| [0009: Digesta rename](0009-digesta-rename/spec.md) | APPROVED | Naming and existing-environment compatibility |
| [0010: preview deployment](0010-backend-preview-deployment/spec.md) | APPROVED | Separate mock-backed server preview |
| [0011: backend dev host](0011-backend-dev-host/spec.md) | APPROVED | Preview API hostname correction to dev.abako.xyz |
| [0012: platform profile](0012-platform-profile/spec.md) | APPROVED | Deployment branding, vocabulary, currency display and compatible custom project types |
| [0013: ink! v5 sandbox](0013-contract-runtime/spec.md) | APPROVED | Staged canonical contract execution with compatibility, persistence and migration gates |
| [0014: evidence commitments](0014-evidence-commitments/spec.md) | APPROVED | Adapter-owned immutable evidence and fixed-size onchain commitments for existing signed evidence operations |
| [0015: adapter/core ownership](0015-adapter-core-boundary/spec.md) | APPROVED | API content/task progress, contract facts/membership and ordinary ink storage |

For a substantial feature, write one note with approval, scope, requirements,
invariants, compatibility, acceptance checks and unresolved decisions. Routine
compatible fixes use a PR. Accepted ADRs and approved requirements remain binding;
record explicit owner approval before changing business rules.

Verification results belong in the PR, with commands, tested revision and limits.
Historical plans and handoffs remain in Git; they are not setup instructions.
