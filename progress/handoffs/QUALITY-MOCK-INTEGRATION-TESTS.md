# QUALITY-MOCK-INTEGRATION-TESTS handoff

Branch: `refactor/mock-integration-tests`.

The three long mock-provider integration scenarios were divided into named
phases for assignments/tasks/settlement, revision/security checks and
planning/milestone/cancellation freeze checks. The HTTP receipt scenario was
split at its authorization boundary, and the file-wide Clippy line-count
suppression was removed. Existing assertions and the
memory/SQLite test matrix remain intact. No production code or contract changed.

Verification on this branch:

- `cargo fmt --all -- --check`: pass after formatting.
- `cargo test -p mock-provider --all-features --locked --test transactions`:
  12 tests pass.
- `cargo clippy -p mock-provider --test transactions --all-features --locked
  -- -D warnings -W clippy::cognitive_complexity -W clippy::too_many_lines`:
  pass without cognitive-complexity warnings.
- `git diff --check`: pass.

The refactor must still be checked on the integrated tree with the simultaneous
mock-domain split and full real-service E2E.
