# SPEC-0013 M3: signed Directory and Calendar

Task: close M3's signed bridge under approved SPEC-0013, SPEC-0003 and SPEC-0006.
M3 is implemented, verified and independently approved within its explicit
Calendar/Directory fixture scope. Rust remains the sole application authority.

Branch `feat/ink-v5-directory-shell`, worktree
`/tmp/kunveno-ink-v5-directory-shell`, base `fa33505` (reviewed Directory slice).
Primary writer: implementation agent. Write scope: standalone SDK Directory ABI
bridge, worker fixture admission/persistence, tests, SDK dependency manifest/lock
and exact-lock policy review, native test-only signed trace, documentation and this
handoff. No canonical contract/artifact, application dependency, business logic,
HTTP/SCALE contract, authority selection, live database or authentication change.

## Result

The explicit signed Directory fixture deploys pinned `directory-v5` and uploads
pinned `calendar-v5`, with immutable fixture configuration. Nine existing commands
run as the shared shell's verified origin. Contracts own catalogs, worker rules,
permissions, moderation and nested Calendar updates. Public IDs come from the
same first 16 bytes of version-1 captured native `StdRng`; ordinary execution
captures inputs once, while a separate trusted comparison method reuses inputs.

Actual nonces, receipts, retained inputs, event cursors, complete runtime and
outcome commit in one SQLite update. Ordinary domain rejection keeps only the
failed receipt/nonce. Native invariant equivalents (`invalid_worker_state`,
`invalid_catalog_state`) and child-instantiation/unknown ABI failures abort without
any shell write. Known top-level metering/trap/language failures retain the M2
failed-operation policy. Queries discard changes. Raw mutation cannot enter a
signed store. Startup verifies root/history, complete runtime identity, pinned
Directory code and a hash of exact genesis fixture configuration (including gas).

A frozen 49-call signed trace uses disposable keys; no private keys are exported.
Rust and real WASM replay it independently. Each step compares outcome plus SHA256
of the full SCALE M3 projection and complete serialized shared shell. This covers
IDs, callers, nonces, receipt bytes, retained inputs/time and exact events/cursors.
Restarts occur mid-trace. Separate tests cover internally captured IDs, requester
visibility, invalid envelopes, replay, incomplete coverage, query isolation,
resource exhaustion, new-child rollback on SQLite failure, competing workers and
startup/configuration failures. ABI unit checks reject mismatched result flags,
unknown host/domain/language results, trailing bytes and malformed recipients.

Authoring is explicit and ignored during ordinary checks: set
`DIRECTORY_SIGNED_TRACE_OUTPUT` to a temporary path and run `cargo test -p
mock-provider --lib export_signed_directory_trace --all-features --locked --
--ignored`. Ordinary tests read committed fixtures and never regenerate them.

## Dependency delta

SDK lock adds rand 0.10.2, rand_core 0.10.1, chacha20 0.10.2 and cpufeatures 0.3.1.
All sources/versions/checksums match the existing app lock exactly. No existing
SDK package is upgraded or removed. Only rand's `std_rng` feature is selected,
with defaults disabled. Current primary registry metadata and published source
were checked: MIT OR Apache-2.0, MSRV 1.85, none yanked. Existing upstream crypto,
SIMD/CPU-detection unsafe and platform/libc implications remain documented in
`tools/ink-sandbox/dependencies.md`; no new native library or handwritten crypto.

The exact-lock policy fingerprint is renewed for this reviewed delta (`0868a865d1445ca6b43f194af80b6920741a22748f80fd1e7db54aa409bebbaf`); the four
advisory IDs, thirteen package/version license exceptions and December 1 expiry
are unchanged. Independent milestone review accepted this exact delta. The app lock,
SDK cohort and canonical contract build locks are unchanged. SDK identity changes
reject old private fixture databases; use fresh disposable candidate data. No
migration or reset of ordinary provider data occurs.

## Verification

- `bash scripts/verify.sh ink-sandbox`: PASS, formatting/check/strict Clippy,
  40 real-runtime/unit/integration tests, all-target build.
- `bash scripts/verify.sh ink-sandbox-dependencies`: PASS, advisories/bans/licenses/
  sources under unchanged approved exceptions and renewed exact lock.
- `bash scripts/verify.sh app`: PASS, formatting/check/strict Clippy, 112 workspace
  tests plus 2 explicitly ignored authoring tests, 2 doctests, isolated memory/SQLite
  checks, all-target build, frontend WASM and all 8 real-service E2E flows.
- `git diff --check`: PASS.

Logs: `/tmp/ink-directory-shell-{sdk-gate,policy,app-gate}.log`. App verification
uses disposable PostgreSQL/SQLite and loopback test services. Root dependency
baseline findings remain recorded and unwaived; SDK policy success does not
claim root policy success. No workflow change or artifact rebuild is required.

## Independent review and remaining gates

Independent xhigh review approved exact implementation
`9dccb981c414d4b06350bdca474366b965fe248b`, with no actionable findings. The reviewer
independently reran 9 unit tests and all 8 signed Directory tests (17 passed),
confirmed the complete SDK/application evidence and accepted the exact dependency
delta and policy renewal. It explicitly approved closing M3 within fixture scope.
The final evidence commit changes documentation only and needs no implementation
re-review. Previous canonical Calendar/Directory source, Unicode and artifact
reproduction reviews remain in the linked M3 slice handoffs.

Partial coverage remains explicit fixtures only. Reputation/matching is M4;
actual Project reference/reservation-controller wiring is M5/M6. Full import,
large-state capacity, measured M0 performance, rollback and whole-provider
promotion remain open. There is no hybrid authority, fallback, public sandbox
endpoint, master merge, tag, publication or deployment in this task.
