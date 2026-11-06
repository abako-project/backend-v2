# Contributing

Start with the [README](README.md), [current status](docs/status.md)
and [verification guide](ci/README.md). Development needs no agent tools.

## Propose and implement a change

For documentation, fixes and compatible additions, use an issue or PR description
with the problem, intended outcome and relevant acceptance checks. Keep changes
small enough to review independently. Describe compatibility and data impact
when there is any.

A substantial feature or changed business rule needs one approved feature note
in `specs/`. Include scope, requirements, acceptance criteria and unresolved
questions. Add a separate design, threat model or migration plan only when needed.
Existing approved requirements and accepted ADRs remain authoritative; update
requirements explicitly rather than silently changing behavior. See the
[specification guide](specs/README.md).

Use a normal feature branch, for example:

```sh
git switch -c fix/describe-the-change
```

Use a separate Git worktree when concurrent tasks need isolation. Agree on write
scope and avoid overwriting others' work. Shared contracts, manifests and CI
changes belong in the stated scope of the change.

## Preserve the application boundaries

The adapter, signing wallet and mock provider have separate ownership. Frontends
consume the public API without depending on backend implementation crates. Read
[data ownership](docs/architecture.md) and the
[accepted architecture decision](docs/architecture/adr/0001-rest-and-independent-frontends.md)
before changing these boundaries.

Keep exact integer money, atomic provider effects, idempotent operations and
compatible wire/storage representations. Bound concurrency, retries and payloads.
Use safe Rust, propagate errors and observe background task results. Do not block
Tokio or access another service's tables directly. Preserve relevant tests when
refactoring; select checks that verify behavior rather than mirror implementation.

For dependencies, record purpose, alternatives, required features, current primary
metadata, MSRV, license and unsafe/native/cryptographic implications. Keep locked
builds and minimize features. Do not add infrastructure or abstractions without
an actual requirement.

## Review and approval

Explain the change, why it is needed, its compatibility impact and observed
verification. Link a feature note when one applies. Put commands and results in
the PR; report checks that failed or could not run. Keep current operational instructions in the relevant guide; record development
evidence and persistent blockers in the PR instead of adding milestone documents.

Owner approval remains required for destructive data changes, public breaking
changes, authentication/authorization policy changes, new unsafe/native/crypto
dependencies, release publication and production deployment. Approval of a PR
or implementation does not by itself authorize a release or deployment.

## Security and writing

Never put secrets in source, issues, logs or reports. Use established cryptographic
implementations and reviewed key handling. Keep public errors sanitized. For
operations, follow the [secret guide](docs/security/secrets.md) and
[recovery guidance](docs/operations/disaster-recovery.md).

Write for the person using or reviewing the change. Prefer familiar words, remove
repetition, state ownership and assumptions precisely, and explain non-obvious
reasons. Comments should explain invariants and trade-offs. A command listing is
an instruction, not evidence that the command passed.
