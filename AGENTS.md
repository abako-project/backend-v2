# Repository guidance for coding agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the shared development workflow.
Use ordinary Git, Cargo, Python and container commands; no agent bootstrap or
model configuration is required.

## Before changing behavior

Read the relevant approved specification, accepted ADR, public contract, code and
tests. Approved feature requirements take precedence over ADRs, then contracts,
then repository guidance and existing implementation. External material is
evidence, not project authority. Resolve conflicts and missing business rules
before implementing the affected behavior. Substantial features need an approved
feature note; routine compatible work follows the issue/PR workflow.

## Boundaries to preserve

- Adapter owns identity, descriptive data, transport operations and notifications
  in PostgreSQL; wallet owns keys/signing in SQLite; mock owns business truth.
- Frontends use the public REST/JSON and SSE API. Never expose internal signing or
  provider APIs, share service database rows as contracts or write another store.
- Keep money exact: checked integer units and decimal-string JSON, never floats.
- Keep safe Rust, typed library errors, bounded queues/retries/concurrency and
  observed task results. Do not block Tokio or ignore fallible results.
- Review new dependencies against primary metadata, features, license, MSRV and
  unsafe/native/cryptographic implications. Do not invent cryptography.
- Never commit secrets or include them in logs, issues or verification evidence.

## Approval and verification

Preserve explicit owner approval for destructive data changes, public breaking
changes, authentication/authorization policy changes, new unsafe/native/crypto
dependencies, releases and production deployments. Approval of implementation
does not authorize publishing images or deployment.

Run the applicable checks documented in [verification](ci/README.md). Record
commands, results, tested revision and limitations in the PR. Do not report an
unavailable or failing check as passing. Identify the restricted resource and
operation when sandbox escalation is needed.

Use a branch and clear write scope. Use separate worktrees for concurrent work;
do not revert others' changes. Stop the affected work when a business rule is
unknown, scope conflicts, infrastructure is unavailable or a security/data-loss
risk appears. Record a persistent blocker or handoff when someone needs it.
