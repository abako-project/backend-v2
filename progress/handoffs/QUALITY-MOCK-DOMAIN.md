# QUALITY-MOCK-DOMAIN handoff

Branch: `refactor/mock-provider-domain`.

The mock provider domain was split into focused command, project, validation and
test modules. The calendar unit tests now live in `calendar/tests.rs`. The
signed command surface, persisted state shape and domain rules were not changed.
The long project dispatcher is now a short router to planning, proposal,
lifecycle, task and milestone handlers; validation has separate worker, project
and history checks.

Verification on this branch:

- `cargo fmt --all -- --check`: pass after formatting.
- `cargo test -p mock-provider --all-features --locked`: 4 unit tests and 12
  integration tests pass; doc tests have no cases.
- `cargo clippy -p mock-provider --all-targets --all-features --locked -- -D warnings`:
  pass.
- `git diff --check`: pass.

The local Rust toolchain printed an intermittent `rustc interrupted by SIGSEGV`
backtrace during the first test and Clippy builds, while the subsequent compiler
and test processes completed with exit code 0. This needs monitoring in later
workspace gates; it is not reported as a clean toolchain run.

No business-rule or wire-contract decision was made here. Full real-service E2E
still needs to run on the integrated tree.
