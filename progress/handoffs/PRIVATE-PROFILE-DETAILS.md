# Private profile details

Approved scope: optional client contact email and descriptive worker experience per catalog role in adapter/PostgreSQL. Backend checkout: master; changes uncommitted for frontend/manual review. Provider matching and payments are unchanged. No separate worktree/runtime was introduced.

Migration 0012 is applied at startup, preserving existing profiles and repeat startups. Owner PUT/GET handles the fields; public projections exclude them. Existing authorized project-participant reads choose client or worker contact email by project role. OpenAPI, profile/participant privacy tests, API fixture seeder, README and REST spec are updated.

Verification: 28 adapter tests and 95 workspace tests passed; fmt, all-target/all-feature check, clippy, doctests and build passed. Local API/Playwright checks passed; adapter image was rebuilt and deployed on the single development stack.

Remaining gate failures: cargo deny rejects existing licenses/maintenance advisories; cargo audit reports three quinn-proto 0.11.17 advisories (RUSTSEC-2026-0338, 0340, 0343). Cargo.lock and dependency policy are unchanged. Logs: /tmp/abako-client-settings-20261013/backend-gate-{6,7}.log. Dependency remediation is separate work; no audit suppression was added.

Development reset/reseed was explicitly authorized after missing temporary secrets. Current secrets/data: $HOME/.local/share/kunveno-dev/abako-auth-review. Ten workers, three coordinators, three clients have at least 1000 mock KVN; contact mail and role metadata come from the API seeder. The frontend's new planned-time budget allocation uses existing coordinatorMinutes and requirement minutes/budget; it does not require a provider contract change.

Manual review evidence and remaining frontend differences: frontend/docs/reviews/profile-contact-experience-hours.md in the frontend repository. No commit or push has been made for this scope. Preexisting .gitignore and unrelated audit documents are preserved and excluded from this change.
