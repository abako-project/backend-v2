# POC-07: Independent backend acceptance verification

Specification: SPEC-0003 (APPROVED).  
Task: POC-07.  
Role: test-verifier.  
Branch: `feat/port-verification`.  
Worktree: `/tmp/kunveno-port-verification`.  
Base commit: `a8d2388` (runtime implementation is unchanged from `a5e0d85`).

## Allowed paths and files changed

Only this report and `SPEC-0001-verification.md`; no implementation, test or contract changes.

## Decision

POC-07 can close for the **local mock-backed proof of concept**. This is not
production approval or verification of real funds, HSM custody, banking, chain
submission, the separate passkey login flow, Leptos, or the cargo-deny policy.

## Commands and observed results

- `cargo build -p wallet -p mock-provider -p adapter-api --all-features --locked`: exit 0, binaries produced from `a5e0d85`. Rust 1.96.1 nevertheless printed a `rustc interrupted by SIGSEGV` LLVM debug-location backtrace during dependency compilation. This is a toolchain anomaly, not a clean build log; the exact cause was not established.
- `python3 -u scripts/poc-e2e.py --binaries target/debug`: exit 0 at `9435897` after the exact-revision service build; **8/8** scenarios passed against disposable adapter PostgreSQL with mock SQLite and memory. No compiler diagnostic appeared during E2E. This run scanned raw REST responses, SSE data lines, custody metrics and bounded service logs for generated secret markers.
- At `a8d2388`, the exact SSE happy-path scenario passed again in both SQLite and memory after the scan was moved before parsing **every** SSE line. Only that two-line test movement changed since the 8/8 run.
- `cargo test -p wallet -p mock-provider --all-features --locked`: exit 0; 39 focused tests passed (10 mock unit, 2 Bramp, 18 mock integration, 9 custody). The same compiler backtrace appeared before test output. A second run gave the same exit/result and diagnostic.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- An earlier E2E run against pre-existing binaries passed two scenarios, then failed `legacy catalog seed differs`. Their exact feature/revision match was not established. Rebuilding the three binaries with `--all-features` from the tested revision removed the failure. Always build the specified binaries before running this script; the earlier result is not evidence of a source-code regression.

## Acceptance trace

The signed E2E exercises planning quote and delivery, atomic upfront funding and
team reservations, mandatory milestone tasks, all-skill matching, four sequential
milestones with teams of 5/3/4/2, continuity, final project completion, exact
payouts/scores, replay after a dropped provider reply, SSE resume, and public
dispute opening with frozen funds. The focused mock tests cover competing
approvals/rollback, authorization, both storage modes and rejected submissions.
This supports PLAN, CAL, MATCH, MILE, MONEY, SCORE, TASK and DOM scenarios in
`specs/0003-transactional-marketplace/acceptance.feature` for this POC.

## Limitations and handoff

`scripts/poc-e2e.py` scans generated master-key, root-seed, service-token and
bootstrap-password markers in REST response bodies, complete SSE lines, custody
metrics, and at most 4 MiB of each service's captured stdout/stderr (including
its tracing output). This is known-marker POC evidence, not proof that all
possible secrets are absent. The user excluded Leptos and cargo-deny from this
backend closeout. `SPEC-0004` still documents passkey login/options blocked by
the email-first identity decision. None of these exclusions is a pass for those
separate capabilities.

Blocker for POC-07: none within the approved local POC scope.
