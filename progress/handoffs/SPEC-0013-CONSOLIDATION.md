# SPEC-0013: consolidated review

Task: SPEC-0013-CONSOLIDATION, approved SPEC-0013. The owner requested one review
per repository, separate commits, and an independent Astra review at xhigh reasoning.
One primary writer owns the consolidation, scoped review fixes and documentation;
Astra has read-only review scope.

Backend: `review/ink-v5-runtime`, `/tmp/kunveno-ink-v5-review`,
[PR24](https://github.com/abako-project/backend-v2/pull/24), base `d4d2597` (`master`).
Canonical: `review/ink-v5-runtime`, `/tmp/kunveno-contracts-review`,
[PR21](https://github.com/abako-dev/contracts/pull/21), base `5c4f344` (`main`).

## History and scope

All 40 original commits in each repository remain ancestors of the consolidated
head. Backend source was complete at `9b0b7d7`; canonical source at `a3d353c`.
Backend M6 documentation `fb458bb` has the identical tree to included `cce1ee0`.
No squash, source-history rewrite or artifact repin was needed. The superseded
backend PRs 13–23 and canonical PRs 13–20 are closed; original branches remain for
pinned source provenance. Dependency updates, v6 and unrelated Clippy work are excluded.

Only these follow-ups were added:

- Backend `a0cffd4`: include every linked mock-provider change in SDK CI and run
  the existing expiring exact-lock SDK policy, using pinned cargo-deny 0.20.2.
- Backend `eef7ad0`: evaluate the metered sandbox primary before the unmetered
  native comparison. Primary infrastructure failure returns before native work;
  Rust/Shadow ordering, captured inputs and atomic writes are preserved.
- Backend `77d6325` and canonical `53102ff`: correct current limits, build commands,
  implemented capabilities and open rollout gates. No contract code, artifacts,
  lockfiles or approved dependency exceptions changed during consolidation.

## Independent review

Reviewer: Astra, xhigh reasoning. Scope: both complete default-branch-to-tip diffs,
approved spec, milestone evidence, goal coverage and unnecessary complexity.
Final runtime review covers `eef7ad0`; current documentation heads are `77d6325`
and `53102ff`. Both P2 findings above are closed after focused re-review.
Verdict: approved as a consolidated private candidate, with no remaining blocking
implementation finding. Documentation review passed at both exact heads above.
This is implementation review, not rollout approval.

| Milestone | Consolidated verdict |
|---|---|
| M0 | Executable compatibility proof accepted; formal performance gate remains open |
| M1 | Shell/native separation and captured inputs accepted |
| M2 | Bounded worker, persistence and failure boundaries accepted |
| M3 | Directory/Calendar parity and signed integration accepted |
| M4 | Exact reputation/matching parity accepted, with documented inherited sampler precision |
| M5 | Project factory, planning and tasks accepted |
| M6 | Execution, money, disputes, evaluations and Bramp accepted |
| M7 | Complete retained-state import, sealing, comparison and current-state rollback accepted |
| M8 | Whole-provider candidate accepted; promotion/observation/retirement incomplete |

Goal assessment: the same canonical v5 WASM runs in one embedded FRAME/wasmi
engine without nodes or block production, preserving the frontend API and native
application behavior within the private candidate scope. No new business-rule,
authorization, money or data-loss blocker was found. The majority of the large
backend diff is committed fixture/artifact data and build lockfiles, rather than
new production runtime logic.

Required staging safeguards remain: native reference/default, whole-provider
selection, bounded worker/gas, exact captured inputs, sealed fresh import,
complete archive validation, immutable artifact identity and separate SDK policy.
Paging, compact indexes and the shared URL validator address measured Kreivo
storage, memory, code-size or execution constraints. They add no node or service.

Optional P3 follow-up: separate current provider artifact/checkpoint identity and
worker API from legacy proof/fixture support. Retain unique fault, ABI, cross-call
and atomicity regressions. This cleanup is deferred rather than adding another
runtime or weakening validation during consolidation.

## Verification

On the ordering fix and consolidation:

- `cargo fmt --all -- --check` in the app and isolated SDK: PASS.
- SDK strict Clippy, all targets/features, locked: PASS.
- `cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked
  sandbox_exhaustion_does_not_execute_native_reference`: PASS. The same test
  fails on the original order (native invocation count 1) and passes after (count 0).
- SDK `cargo test --manifest-path tools/ink-sandbox/Cargo.toml --locked
  --test migration whole_ -- --nocapture`: all 5 PASS. Covers signed continuation,
  exact complete archives/receipts, all authorities, domain rejection, replay,
  failure/no-write, restart, SQL rollback, competition and incomplete comparison.
- actionlint 1.7.12 and `bash scripts/verify.sh ink-sandbox-dependencies`: PASS.
  Existing advisory/license/duplicate warnings remain within the approved SDK policy.
- `git diff --check` against both baselines: PASS. All80 original commits are
  preserved; alternate M6 evidence trees compare equal. Original master stays clean.

Local logs: `/tmp/ink-consolidation-reference-order-{before,after}.log`,
`/tmp/ink-consolidation-order-clippy.log`, `/tmp/ink-consolidation-whole-tests.log`,
`/tmp/ink-consolidation-actionlint.log`, `/tmp/ink-consolidation-sdk-policy.log`.

Prior candidate evidence remains in [M8](SPEC-0013-M8.md): full application and
72-test SDK gates, gateway flows in all authorities, offline current-state
rollback, source/artifact reproduction and canonical tests. Those broad suites
and shared-host timings were measured before the ordering fix, not remeasured
as part of this consolidation. No new performance acceptance claim is made.
Root dependency-policy baseline failures remain unwaived; nextest/audit tools
were unavailable. GitHub checks must be read separately from these local results.

## Open gates

M0 formal performance acceptance and M8 promotion, two observed stable release
cycles and eventual duplicate-rule retirement remain open. The inherited v1 tie
sampler retains the documented minute bias; no input semantics were changed.
Directory WASM is 192,755 bytes, with 3,853 bytes below the current code cap.
Rust remains default. This task merges no default branch, publishes no release,
deploys nothing and changes no authority. No implementation blocker remains.

## Evidence follow-up

SPEC-0014 supersedes the URL-helper design described in the original milestones.
The current candidate stores fixed evidence commitments and executes three
contracts; References is removed. Original signed URLs and complete rollback
remain supported through an offchain SDK compatibility map. Astra xhigh approved
both implementation milestones, including compatibility and scope. The full app
and SDK gates, fresh artifact reproduction and twelve authority gateway flows
were rerun successfully. Exact revisions, commands, review findings and remaining
limits are recorded in [SPEC-0014.md](SPEC-0014.md). The original milestone
provenance above remains unchanged.
