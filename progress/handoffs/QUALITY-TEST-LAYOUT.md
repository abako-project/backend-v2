# QUALITY-TEST-LAYOUT handoff

## Task

Move unit-test implementations out of owned production modules into colocated
`tests.rs` modules without changing production behavior or public APIs.

## Delivery

- Branch: `refactor/test-layout`
- Worktree: isolated temporary worktree
- Specification: mechanical quality task; no business rules changed
- Scope: domain primitives, generated contracts, Leptos helpers, and four
  inactive scaffold crates

Each affected `lib.rs` now contains only `#[cfg(test)] mod tests;`. Existing test
bodies live in the sibling `src/tests.rs` file and retain private parent-module
access through `use super::*`.

## Verification

Passed:

```text
cargo fmt --all -- --check
cargo test --locked -p domain-primitives -p generated-contracts -p leptos-web
cargo clippy --locked -p domain-primitives -p generated-contracts -p leptos-web --all-targets -- -D warnings
```

The four inactive scaffold crates are not workspace members. Their library test
targets were compiled and run directly with `rustc --test`; all four tests passed.

## Risks and blockers

None. This change only relocates tests.
