# Feature notes and specifications

Existing approved specifications define product behavior; find them in
[the index](../SPECS.md). Accepted amendments and ADRs remain authoritative.
Historical verification establishes results only for its recorded revision.

For a substantial feature, add one `NNNN-kebab-case-title/spec.md` containing:

- Status (`DRAFT` or `APPROVED`) and the approval source.
- Problem, scope and explicit exclusions.
- Requirements, invariants and failure/compatibility behavior.
- Acceptance checks and unresolved business questions.

Implementation starts after approval and resolution of blocking questions.
Add separate architecture, security, migration or scenario documents when they
help assess the change; there is no mandatory seven-file bundle. Routine fixes,
documentation and compatible additions can use the issue or PR description under
[CONTRIBUTING.md](../CONTRIBUTING.md).

Keep existing approved feature files and accepted decision history. Record new
approval explicitly when requirements change; do not treat a code change as
implicit product approval. Verification results normally belong in the PR.
