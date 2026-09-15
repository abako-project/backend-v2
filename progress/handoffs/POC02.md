# POC-02: transactional mock provider

Task: implement the authoritative local smart-contract simulator.
Specifications: APPROVED SPEC-0001, SPEC-0002, SPEC-0003, including MONEY-003A
approved on 2026-09-09 in the integration tree; ADR-0001 and frozen generated contracts.
Branch: `feat/poc-mock`. Worktree: `historical mock worktree`.
Write scope: `services/mock-provider/**` and this handoff. Shared lockfile excluded.

## Delivered

- Authenticated internal HTTP discovery, nonce, signed call, receipt, snapshot and event routes.
- Verified immutable SCALE/sr25519 calls, generation binding, exact replay before freshness checks,
  and atomic nonce/receipt/events plus all business effects.
- Interchangeable memory and SQLite features; SQLite default and optional missing-only catalog seed.
- Provider-owned workers/calendars, indexed all-skill assignment, independent mode reputations,
  weekly reservations, planning negotiation, proposals/task storages, tracking, escrow and settlement.
- Client or assigned coordinator may cancel or dispute; other workers cannot. No automatic
  fund release, reservation release, arbitration or reversal of settled payments.

## Observed verification (2026-09-10)

All commands run in the task worktree:

```sh
rtk proxy cargo fmt -p mock-provider -- --check
rtk proxy cargo clippy -p mock-provider --all-targets --all-features --offline -- -D warnings
rtk proxy cargo test -p mock-provider --all-features --offline
rtk proxy cargo test -p mock-provider --no-default-features --features storage-memory,mock-seed --offline
rtk proxy cargo test -p mock-provider --no-default-features --features storage-sqlite,mock-seed --offline
rtk proxy cargo check -p mock-provider --no-default-features --features storage-memory --offline
rtk proxy cargo check -p mock-provider --no-default-features --features storage-sqlite --offline
rtk proxy git diff --check
```

All returned zero. All-features: 4 unit and 12 integration tests passed; memory-only:
3 unit and 5 integration tests; SQLite-only: 4 unit and 7 integration tests.
Clippy reports the inherited root `clippy.toml` MSRV mismatch, already assigned to the integrator.

Coverage includes full negotiated lifecycle and exact settlement, concurrent approvals and rollback,
capacity preservation, role-independent all-skill matching, exact replay and revision guards,
delegated scores, mode-specific reputation selection, both authorized dispute/cancel actors,
unauthorized workers, frozen funds, retained generation across independent SQLite runtimes,
persisted-state validation, and real signed HTTP/authentication/event replay.

## Limits and integration

SQLite stores the complete validated aggregate in one transactional row using `BEGIN IMMEDIATE`;
memory serializes one runtime. This deliberate POC ceiling is documented in source; it is not
production storage or a claim that a future blockchain migration only changes an endpoint.
The adapter remains responsible for filtering the private snapshot and notification recipients.
Full adapter/custody/frontend end-to-end and workspace gates belong to the integrator.
Codebase Memory indexed this worktree, but symbol searches did not cover these untracked files;
exact-file reads were used as the documented fallback. No remaining blocker in this scope.
