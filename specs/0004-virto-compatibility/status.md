# Specification status

State: APPROVED

Approved by the product owner on 2026-09-23 through the backend-port plan and
the subsequent scope decisions. Mock Bramp and server-side WebAuthn credential
ceremonies are integrated in the backend branch. On 2026-09-25 the owner
approved username-first passkey login without email verification for the
product's current phase; `spec.md` and `acceptance.feature` record that rule.
AUTH-001 username-first login is implemented and verified with a virtual
authenticator. Workspace tests passed 68/68 and the backend E2E passed 8/8
with SQLite and memory mock modes. The
[`AUTH-001` handoff](../../progress/handoffs/AUTH-001.md) records the remaining
credential-enumeration decision; this verification is not a production
security certification.

The mock-only contract is in `spec.md`; `acceptance.feature` records observable
cases. `plan.md` is historical source inventory and must not override the spec.
Governance, DAO, real Kreivo calls, banking and standalone payments are excluded.
