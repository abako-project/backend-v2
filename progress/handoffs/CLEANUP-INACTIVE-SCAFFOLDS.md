# Inactive scaffold cleanup

## Task

Remove the empty packages identified by the owner. This is maintenance, not a
change to an approved business specification. Base commit: `acdf4e2`. Branch:
`chore/remove-inactive-scaffolds`; work was isolated in a temporary worktree.

## Changes

- Removed nine unused `services/*` binaries, the unused notification worker and
  four `crates/*` libraries containing only Cargo defaults. None was a workspace
  member or a deployed process.
- Removed the obsolete generic project bootstrap scripts and their four private
  templates so they cannot recreate placeholder packages. The template-era
  manifest and checksum inventory were removed because they did not describe
  this product's current files.
- Updated the README and current workspace/porting guides. Historical handoffs
  remain historical records.

The six active workspace packages, public contracts, runtime code and lockfile
were not changed. No business rule or service boundary was introduced.

## Verification

Passed: `bash scripts/verify-template.sh`, `cargo metadata --no-deps`,
`cargo fmt --all -- --check`, `cargo check --workspace --all-targets --all-features
--locked`, `cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings`, `cargo nextest run --workspace --all-features` (46/46), `cargo test
--workspace --doc`, `cargo build --workspace --all-targets --all-features
--locked`, and `git diff --check`.

The first Nextest attempt could not open local sockets inside the sandbox;
the unchanged suite passed when run with local socket access. No source test
was added because this cleanup removes code rather than changes behavior.

## Risk

The old generic bootstrap workflow is intentionally gone. New services must be
created through an approved task instead of by choosing a name in a template.
