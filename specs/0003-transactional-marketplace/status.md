# Specification status

State: APPROVED

Approved by the product owner on 2026-09-08: architecture review, REST/frontend isolation, reputation defaults, and negotiated planning plus explicit milestone prices. Latest instruction: implement.

Last updated: 2026-09-24.

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
- The owner removed Leptos and cargo-deny from this POC's backend acceptance
  gate. The previous cargo-deny failure remains recorded evidence, not a pass.
  It must be revisited before a production or published dependency-policy claim.

Milestone rejection and formal public dispute opening now follow SPEC-0005:
current rejected submission, one response and a whole-project freeze. The old
milestone dispute command/route was removed. Planning disputes retain their
existing route. Arbitration, refunds and fund release after resolution remain
outside the approved POC scope.

Requirements remain APPROVED. Full verification and task closure remain open.

Port review: the owner requested legacy-scale E2E coverage on 2026-09-15. The
script now includes four milestones with teams of 5/3/4/2 in a separate scenario;
the original security/SSE flow is retained. At that verification point, detailed
profiles, catalog associations, explicit project completion and sequential
activation were non-equivalent at that time. The 2026-09-24 backend integration
implements these approved changes; the historical gap is closed in the branch. See
`../../docs/project/porting-coverage.md`. This test expansion does not close
independent verification.

The owner approved a further amendment on 2026-09-23: team continuity without
`assignmentKey`, a task in every milestone before proposal submission,
sequential activation despite atomic upfront assignment/reservation, and
automatic project completion after the final accepted milestone. These rules
are in `spec.md` and `acceptance.feature` and implemented in the backend branch.

Expanded E2E verified on 2026-09-15: both original and multi-team scenarios passed
against both SQLite and memory after rebuilding current service binaries. Evidence:
`../../progress/handoffs/E2E-legacy-scale.md`.

The 2026-09-23 signed E2E passed single-milestone, four-milestone and dispute
scenarios on both backends; workspace tests reached 49 passing. This does not
close POC-07's independent verification. The owner accepted deferring the two
Leptos maintenance advisories for this POC. The checked-in dependency policy has
not been changed or passed; it is outside this backend closure.

On 2026-09-24 the expanded signed E2E passed the sequential four-milestone
5/3/4/2 scenario against adapter PostgreSQL with both mock memory and SQLite.
The fixture checks mandatory tasks, stable storages, team continuity,
one active milestone at a time, final project completion, payouts and scores.
This is implementation evidence, not independent POC-07 closure.
