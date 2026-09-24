# VIR-01: WebAuthn passkeys

Spec: `specs/0004-virto-compatibility/spec.md` (AUTH-001, APPROVED).
Branch: `feat/port-passkey`. Worktree: `/tmp/kunveno-port-passkey`.

Implemented in scope: adapter credential and ceremony migration, WebAuthn
registration with password reauthentication, credential listing/removal, login
ceremony and session creation, server-side single-use challenge storage,
five-minute expiry, account/session binding, origin/RP and user-verification
checks, credential uniqueness and sign-counter update. The passkey never sees
the signing seed. Password login and custodial signing remain unchanged.

Dependency review: `webauthn-rs =0.5.5` is the reviewed safe RP wrapper,
MPL-2.0, MSRV 1.88. Only `danger-allow-state-serialisation` is enabled, solely
to persist the ceremony state in PostgreSQL; the state is never returned to
the browser. It uses OpenSSL via `webauthn-rs-core`. The test-only
`webauthn-authenticator-rs =0.5.5` enables `softpasskey` and also uses
OpenSSL. A hand-written verifier was rejected. Registry and API were checked
against crates.io and docs.rs on 2026-09-24.

Verification: `cargo check -p adapter-api --all-targets` passed; both virtual
authenticator tests passed; the PostgreSQL challenge ownership, expiry and
replay test passed against a disposable local cluster. `cargo fmt --all`
completed. The temporary cluster is stopped. The lockfile update was tested
then restored; the integrator owns `Cargo.lock`.

Integration hooks: execute `0003_passkeys.sql` in `App::new`, validate
`auth::passkeys::configured_webauthn()` at startup, route register options,
verify, list, removal and login verify to `auth::passkeys`, and allow the two
public login endpoints through the boundary. The `begin_login(app, principal)`
function supplies login options after a trusted principal lookup.

Blocker: principals currently have usernames but no email. AUTH-001 requires
an email-first public lookup and indistinguishable failures for absent
accounts/credentials. No email field or lookup was invented. Product decision
and an approved account-email policy are needed before exposing login/options
or declaring VIR-01 complete. The existing HTTP contract and happy path are
untouched by this branch.
