# Specification status

State: APPROVED

Approved by the product owner on 2026-09-08: architecture review, REST/frontend isolation, reputation defaults, and negotiated planning plus explicit milestone prices. Latest instruction: implement.

Last updated: 2026-09-23.

Implementation: POC-00 through POC-06 are integrated into `master` at `ed6e4f8`.
All six temporary task worktrees were checked and removed
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

Milestone rejection and formal public dispute opening now follow SPEC-0005:
current rejected submission, one response and a whole-project freeze. The old
milestone dispute command/route was removed. Planning disputes retain their
existing route. Arbitration, refunds and fund release after resolution remain
outside the approved POC scope.

Requirements remain APPROVED. Full verification and task closure remain open.

Port review: the owner requested legacy-scale E2E coverage on 2026-09-15. The
script now includes four milestones with teams of 5/3/4/2 in a separate scenario;
the original security/SSE flow is retained. Detailed profiles, catalog associations,
explicit project completion and sequential activation remain non-equivalent.
See `../../docs/project/porting-coverage.md`. This test expansion does not change
approved domain rules or close independent verification.

Expanded E2E verified on 2026-09-15: both original and multi-team scenarios passed
against both SQLite and memory after rebuilding current service binaries. Evidence:
`../../progress/handoffs/E2E-legacy-scale.md`.

The 2026-09-23 signed E2E passed single-milestone, four-milestone and dispute
scenarios on both backends; workspace tests reached 49 passing. This does not
close POC-07's independent verification. The owner accepted deferring the two
maintenance advisories for this POC. The checked-in dependency policy has not
yet been changed or passed with that decision.
