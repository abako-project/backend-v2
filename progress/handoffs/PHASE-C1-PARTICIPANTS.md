# Phase C1 / BE-C3 handoff

Date: 2026-10-08. Base: master 385124f. Changes are uncommitted pending screen review.

Approved scope: allow proposal submission with empty milestone task storages; authenticated read of client/coordinator public identities under current project participant visibility. No migration. No private contact email disclosure. No changes to matching, reservations, planning settlement, execution approval, completion reviews or payment rules.

New route: GET /api/projects/{projectId}/participants. Reuses account-principal mapping and public profile projection. Response contains client/coordinator account IDs, optional display names/public profiles and image-availability flags. Unknown adapter principal produces null identity/profile, with actual account ID retained. 401 without session; 404 for outsider/missing project; 503 for unavailable provider.

Verified: 77 workspace/all-feature tests, strict Clippy, fmt, check, build and 2 doc tests. OpenAPI references/security passed. Permission/privacy test covers client, coordinator, task-assigned worker, outsider, anonymous, missing/unmapped/replaced participants and unavailable provider. Memory/SQLite empty-storage tests cover submission, separate planning acceptance/execution approval, completion and payments.

Runtime: base repository Compose project abako-auth-review, gateway 8088. Updated provider and adapter images deployed, existing data retained. Live Playwright API check submitted two empty storages successfully, retaining IDs and zero execution escrow; disposable project 0x7764d606aa3a7933567900b093965d17 was then cancelled. Other actor checks: client2/coordinator2 200; client3 404; anonymous 401. No admin secrets involved.

Full screen review and changed frontend modules: frontend worktree docs/reviews/project-C1.md. C2–C9 remain separate tasks. Existing cargo-deny license/advisory failures remain; no dependencies changed. Unrelated .gitignore/local review files were preserved and are outside this scope.
