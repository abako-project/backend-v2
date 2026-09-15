# Specification status

State: APPROVED

Approved by the product owner on 2026-09-08: architecture review, REST/frontend isolation, reputation defaults, and negotiated planning plus explicit milestone prices. Latest instruction: implement.

Last updated: 2026-09-15.

Implementation: POC-00 through POC-06 are integrated at `3ab2b58` on
`feat/rust-rest-poc`. All six temporary task worktrees were checked and removed
after confirming integration; none contains outstanding implementation work.

Recorded verification: 46 workspace tests, memory-only and SQLite-only mock tests,
signed real-service flows for both backends, frontend browser checks and full
Compose/Nginx verification. See `../../progress/handoffs/REST-POC-foundation.md`
and the component handoffs. These are existing execution records, not a claim
that every acceptance scenario has independent sign-off.

Remaining closure work:

- POC-07: final independent acceptance/security verification and its handoff.
  `POC07-frontend-build.md` records a build-tool correction, not this task's completion.
  A read-only evidence review identified missing diagnostic secret-marker evidence
  and an encryption-key rotation scope discrepancy in SPEC-0001. See its status.
- Dependency policy: cargo-deny 0.20.2 was executed on 2026-09-14 and returned
  exit 5. Advisories and licenses failed; bans and sources passed. The blockers
  are CC0-1.0/BSL-1.0 allowances and the two documented unmaintained Leptos
  dependencies. No exception has been applied.

Disputes: opening planning/milestone disputes, participant authorization,
recorded reasons and frozen funds are implemented and covered by provider tests.
Arbitration, refunds and fund release after resolution remain explicitly outside
the approved POC scope; they are not unfinished implementation tasks for this spec.

Requirements remain APPROVED. Full verification and task closure remain open.

Port review: the real-service E2E passed again on 2026-09-15 for SQLite and memory,
but has one milestone/worker. Multi-team legacy coverage, detailed profiles, catalog
associations, explicit project completion and sequential milestone activation are
not equivalent in the Rust port. See `../../docs/project/porting-coverage.md`.
These findings do not silently change approved requirements.

The owner accepted deferring the two maintenance advisories for this POC. The
checked-in dependency policy has not yet been changed or passed with that decision.
