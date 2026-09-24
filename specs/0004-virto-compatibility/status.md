# Specification status

State: APPROVED

Approved by the product owner on 2026-09-23 through the backend-port plan and
the subsequent scope decisions. Mock Bramp and server-side WebAuthn credential
ceremonies are integrated in the backend branch. Passkey login/options is still
blocked: AUTH-001 requires email-first lookup, while principals currently have
only usernames. Independent verification is pending; approval does not imply
delivery of the blocked flow.

The mock-only contract is in `spec.md`; `acceptance.feature` records observable
cases. `plan.md` is historical source inventory and must not override the spec.
Governance, DAO, real Kreivo calls, banking and standalone payments are excluded.
