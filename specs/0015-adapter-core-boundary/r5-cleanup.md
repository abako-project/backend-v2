# R5 cleanup and recovery inventory

Implementation candidate under approved SPEC-0015; verification and exact reviewer
acceptance are recorded in [the handoff](../../progress/handoffs/SPEC-0015.md).
No deployment, live backfill, authority promotion or release is implied.

## Removed code and surviving checks

Paths below are relative to `tools/ink-sandbox` unless marked canonical.

| Removed component | Former caller / purpose | Current owner or regression |
|---|---|---|
| Canonical `storage-v5` | Former Calendar/Directory/Project blob paging and Text | All three now use ordinary ink storage; `calendar_storage`, `directory_storage`, `project_storage`, typed-import unit tests, full retained-state migration and frozen predecessor rollback |
| Canonical `calendar`, `ratings`, `projects`; their three `.contract` files, build locks and bundle entries | M0 VM proof, old Calendar bridge and raw worker tests | Three current application contracts; existing probe for low-level VM behavior. Immutable source/artifact provenance below |
| `src/worker/signed.rs`, CalendarFixture/FixtureWorker, Calendar mode/constructor/dispatch | Partial whole-hour-only signed Calendar bridge | Current Directory/whole execution uses the same real provider-execution shell; no public REST/SSE or application authority changes |
| `tests/signed.rs`: success/replay/restart | Captured IDs/inputs, origin, expired replay, child storage, generation | `directory_signed::captured_ids_replay_read_isolation_and_request_visibility` now explicitly verifies expired replay before/after restart, seed/origin/generation; native signed trace compares complete shell and domain |
| `tests/signed.rs`: invalid envelopes / incomplete coverage | Forged/expired/wrong-generation/wrong-nonce calls; unsupported partial bridge | `directory_signed::envelope_and_incomplete_coverage_failures_do_not_write_or_consume_nonces`. The old fixture's whole-hour restriction is intentionally retired; ordinary integer minutes remain supported |
| `tests/signed.rs`: domain/gas rejection | Failed receipts consume nonce without business effects | Directory `domain_failure_commits_only_failed_receipt_and_invariant_failure_commits_nothing` and `metered_failure_commits_failed_receipt_without_business_changes`; financial metered rollback remains |
| `tests/signed.rs`: SQL failure | Runtime, receipt, events and nonce rollback; retry/restart | Directory `sql_failure_rolls_back_new_child_contract_shell_and_retry`, strengthened with restart and expired replay; Project factory and financial settlement SQL failure/retry tests remain |
| `tests/signed.rs`: competition | One commit, conflicting operation/nonce | Directory `competing_workers_replay_one_commit_and_reject_conflicting_nonce`, now explicitly checks idempotency conflict |
| `tests/signed.rs`: mode/root protection | Reject raw mutations, wrong root/mode, no reseed | Directory `startup_checks_fixture_identity_mode_and_contract_pointer`, now explicitly checks raw mutation, root and unchanged reopen |
| `compatibility::production_wasm_cross_calls_assets_reverts_and_full_restart` | Old M0 scope/payment/rating behavior | Current `financial_signed` full native trace, settlement SQL rollback/restart and approval resource failure, plus `projects_signed` factory rollback and `migration` exact retained state. Old scope API and legacy keep-alive payout formula are not current business semantics |
| Old Calendar use in `tests/worker.rs` and remaining compatibility test | A successful mutation for persistence, query isolation, cancellation and failure checks | Minimal owner-checked `sandbox-probe::set_value`; all six worker cases remain, with identical shell/revision/checkpoint assertions. Trap, nested REVERT, gas and language-error probes remain |

The current bundle is exactly Directory, Calendar, Project and one private test
probe. No runtime limits, dependency families, application permission rules,
service, compatibility registry or storage framework are added.

## Provenance

The last source revision containing the removed canonical directories is
[a0c881f](https://github.com/abako-dev/contracts/tree/a0c881fba4aee54af5aebd37e75a429838b7e8d0).
Their artifact hashes and exact locks remain in
[backend8e5cece](https://github.com/abako-project/backend-v2/tree/8e5ceceba208e8577e86f52d25ac905d0d256d0e/tools/ink-sandbox/fixtures).
These commits and earlier accepted branches are retained. Deployed legacy
contracts are unaffected. Frozen accepted executable manifests are retained
separately; Git provenance is not a substitute for the tested rollback binary.

## Deliberately retained

- Native financial reference, original signed corpora, receipt/history decoding
  and exact replay. V5 is the current state shape, not permission to reinterpret
  old signatures or delete accepted history.
- Explicit paused V4↔V5 paired owner exchange, including API task content, current
  IDs/assignees/counter, frozen case snapshots and reconciliation. Historical
  converters are offline owner tools, not live fallback or parallel writers.
- Fixed evidence ABI conversion and verified legacy preimages. They are needed
  for old evidence signatures, complete current-state export and rollback; they
  are not the removed root-blob codec or duplicated descriptive storage.
- PostgreSQL historical rows, original operations, notifications and immutable
  snapshots. No production database was accessed or backfill claimed complete.

Current public composition already rejects missing/tampered selected content and
has no R1 child-copy/text fallback. `content_copy_tests` proves obsolete copy
writes are not needed; `task_view_tests` proves frozen views cannot re-enable the
predecessor fallback. `backfill-provider-content` verifies selected views via
provider HTTP. Activation remains gated by complete owner capture/verification;
cleanup does not bypass that gate or delete any record.

## Fresh cohort and backup/restore

Removing unused bundles changes the pinned manifest identity, even though the
three application WASM artifacts are byte-identical to accepted R4. Existing
physical checkpoints must reject this identity. Pause/drain, export current V5,
import into a fresh store, compare the complete archive and reopen before routing.
Retain the prior binary and current paired API/core backups. Rollback exports
**current** state after any accepted writes; never restore stale pre-cutover state.

`storage_cohort` verifies current Directory/Calendar/Project writes in Rust,
Shadow and Sandbox, import into the frozen R4 Project executable, reopen, expired
replay and further signed work. It also verifies current code rejects that old
physical checkpoint, then imports its latest archive into a fresh current store,
reopens and replays without changes. The explicit frozen-binary test is run
separately from the normal suite; its ignored status alone is not evidence.

For V4 ownership rollback, preserve both API content and provider facts using the
[task procedure](../../docs/operations/task-cutover.md). This R5 cleanup keeps that
accepted R3 path unchanged. Controlled-host timings, unwaived SDK dependency
findings, independent frontend deployment and observed releases remain open.
