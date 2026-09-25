# AUTH-001 handoff: username-first passkey access

Spec: [SPEC-0004](../../specs/0004-virto-compatibility/spec.md). Branch:
`feat/auth-001-username-passkey`; task worktree: `/tmp/kunveno-auth-001`.

The owner approved username as the account identifier on 2026-09-25. Email
verification is not required. Password login remains available. A registered
WebAuthn credential starts a public login with the normalized username and
finishes in the same principal, custodial wallet, session cookie and CSRF
contract. The adapter still signs provider requests with the custodial wallet;
the passkey does not sign chain transactions.

Changed: `services/adapter-api/src/auth/passkeys/mod.rs`, `http.rs` and
`tests.rs`; `contracts/openapi.json` and its contract test; SPEC-0004
acceptance/status; README and happy-path/coverage guides. Existing PostgreSQL
ceremony storage, RP/origin configuration and credential verification are
reused. No database migration, dependency or external provider was added.

Verification on this branch: format, workspace check, Clippy with warnings
denied, workspace build and doctests passed. Nextest passed 68/68 tests against
temporary PostgreSQL, including the virtual-authenticator HTTP test for
same-account login, uniform failure for unknown/invalid/no-passkey usernames,
and rejected challenge replay. The backend E2E passed 8/8 scenarios: the
signed lifecycle, four-milestone teams, dispute and auxiliary flow on each of
the SQLite and memory mock backends. The E2E checks passkey registration; the
full passkey login is covered by the dedicated virtual-authenticator HTTP test.

Product limit: successful login options contain credential identifiers, so
an anonymous caller can distinguish a username with passkeys from one without.
Uniform 401 responses only cover unknown, malformed and no-passkey names.
The owner accepted retaining this working flow for the current phase. Hiding
passkey enrollment would need a later explicit requirement and design. This is
not evidence of production readiness: rate limiting, real-asset custody and
HSM rotation require their own security gates.
