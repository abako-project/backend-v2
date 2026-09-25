# Specification status

State: APPROVED
Independent acceptance verification: CLOSED on 2026-09-25.

Approved by the product owner on 2026-09-23. Profiles in adapter PostgreSQL,
catalog associations and requests in the mock, and canonical task-storage routes
are integrated. The owner-requested legacy profile-image parity is included as
`PAR-MEDIA-01`; the legacy nested task read in `PAR-STORAGE-01` was corrected to
the implemented canonical read plus project projection.

Black-box acceptance passed in both mock storage modes against real adapter,
wallet and provider processes with disposable PostgreSQL: eight E2E scenarios,
including four milestones, profile images, catalog decisions, task ownership,
disputes and SSE. The workspace suite passed 69/69 tests, including adapter
restart/notification persistence and the image authorization/size test.
OpenAPI references/security, formatting, check, Clippy and build passed.
`cargo-deny` and Leptos-specific acceptance are outside the owner's backend
gate. Evidence and legacy-test limitations are in
`progress/handoffs/SPEC-0006.md`.
