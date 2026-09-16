# E2E legacy-scale coverage

Date: 2026-09-15. Branch: `feat/virto-plan-e2e`. Worktree: repository root.

Task: preserve the original signed-service test and add the requested four
milestones with teams of 5/3/4/2. Authority: SPEC-0003 and the owner's explicit
coverage request. No Rust domain behavior or public API changed.

Changed files: `scripts/poc-e2e.py`, README, porting coverage, SPEC-0003 status/tasks.
The separate SPEC-0004 plan and specification index cover future Virto work, not
implementation delivered by this test.

## Evidence

- `cargo build -p adapter-api -p wallet -p mock-provider --all-features --locked`: passed.
- `python3 -m py_compile scripts/poc-e2e.py`: passed.
- `python3 scripts/poc-e2e.py`: exit 0; original and multi-team scenarios both
  passed on SQLite and memory. Local socket access required execution outside
  the sandbox; the first sandbox attempt failed with socket permission denied.
- `git diff --check`: passed.

The expanded scenario creates 10 workers (5 unavailable), 2 eligible coordinators
and one client. It checks all-skills matching without role matching, exact team
sizes, distinct task storages retained through draft edits and execution approval,
task progress, completion requests, payouts and escrow conservation after each
acceptance, idempotent replay, minute-weighted reputation, final delegated team
rating and per-week consumed capacity. The unused coordinator gains no commitment
or reputation. Maps to MATCH-001, CAL-002/003, PLAN-002/005, TASK-002,
MILE-001, MONEY-002 and SCORE-001/002.

The original scenario still checks signatures through custody, access/CSRF/CORS
denials, lost-response recovery, password changes, notification read state and SSE.

## Limits

This is backend E2E, not a browser full-stack test. It does not restore legacy
assignment-key continuity, sequential activation or explicit project completion.
No new dependencies or production integration. Full workspace/security gates were
not repeated for this Python/documentation-only change; POC-07, TASK-005 and their
documented dependency/rotation issues remain open. SPEC-0004 remains DRAFT.
