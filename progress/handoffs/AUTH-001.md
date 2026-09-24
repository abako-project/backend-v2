# AUTH-001 handoff: passkey login identity

Spec: [SPEC-0004](../../specs/0004-virto-compatibility/spec.md). Branch:
`feat/backend-port-close`. Worktree: `/tmp/kunveno-backend-integration`.

Registration, credential storage, removal, assertion verification and the
existing session integration are implemented in `services/adapter-api/src/auth/passkeys/`,
the adapter routes and `migrations/0003_passkeys.sql`. PostgreSQL persists
ceremonies. Virtual-authenticator tests cover the implemented ceremonies;
`cargo test --workspace --all-features --locked` passed 67 tests and the
mock-backed E2E passed 8/8 scenarios. The E2E checks the passkey registration
boundary, not a complete passkey login.

`POST /api/auth/passkeys/login/options` is deliberately not exposed, so a
passkey-only login cannot start. The approved spec requires lookup by account
email, but principals have usernames and no unique verified email. Creating a
public email lookup or treating an unverified address as an account identifier
would add an unapproved authentication policy and risk account enumeration.
The public verify route cannot complete a login without issued options.

Decision needed from the owner: add unique verified email to principals with
an explicit verification flow, or amend AUTH-001 to use username for this local
POC. Preserve uniform public failures either way. Then implement login/options,
test a complete same-account passkey login with a virtual authenticator, and
update OpenAPI and the happy-path guide. Do not mark SPEC-0004 delivered before
that flow passes. No real assets may be used with this POC.
