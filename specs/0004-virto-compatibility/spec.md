# SPEC-0004: Mock Virto auxiliary interfaces

Status: APPROVED — product owner approved implementation on 2026-09-23.

## Boundary

The adapter exposes typed REST/JSON operations to both frontends. The mock owns
business state and signs/executes commands through the existing custody path.
Nothing in this specification submits to Kreivo, deploys a contract, talks to a
bank, implements governance or resolves a dispute. `wss://kreivo.io` is a
documented future integration target, not a usable provider switch.

## AUTH-001: Additional passkey login

An existing password-authenticated principal may register FIDO2/WebAuthn
credentials after a fresh password check. Password login remains available.
Registration and removal require the principal's session and password; no
credential may be linked to another principal. Login starts with the existing
account username, normalized exactly as password login, and ends with a verified
assertion that creates the same opaque session and CSRF contract as password
login. The existing custodial wallet and `AccountId32` do not change. The
browser never receives a private signing seed.

The server generates unpredictable, single-use challenges with a five-minute
expiry, stores ceremony state server-side, validates RP ID, configured origin,
challenge, credential ownership, signature and user verification, and rejects
replay and stale ceremonies. `WEBAUTHN_RP_ID` and `WEBAUTHN_ORIGIN` come from
environment configuration; Compose defaults to `localhost` and
`http://localhost:8088`. A non-local deployment requires an explicit HTTPS
origin. Never infer the trusted origin from request headers. There is no
WebAuthn-to-chain signature or `Pass.register` in this phase; custody still
signs provider commands with sr25519.

Public routes: `POST /api/auth/passkeys/login/options` and
`POST /api/auth/passkeys/login/verify`. Authenticated routes:
`POST /api/auth/passkeys/register/options`,
`POST /api/auth/passkeys/register/verify`, `GET /api/auth/passkeys`, and
`POST /api/auth/passkeys/{credentialId}/remove`. Options and verification
requests have typed JSON bodies; removal requires the password again.
Unknown usernames, invalid usernames and existing accounts without a passkey
return the same public authentication error. A login challenge remains bound to
the named principal; a credential belonging to another principal cannot use it.
For this phase, a username with registered passkeys receives options containing
credential identifiers. An unauthenticated caller can therefore infer that the
account has passkeys. The owner accepts this disclosure to retain the working
username-first flow; hiding passkey enrollment requires a separate requirement
and design. Credential identifiers do not grant authentication.

## RAMP-001: Simulated deposit

The account owner creates a deposit request for a strictly positive integer
amount of asset 1 (KVN). The owner, destination custodial account and amount
are fixed at creation. No spendable balance changes yet. A separate system
origin confirms it once, crediting exactly that amount to the existing mock
ledger. Confirmation cannot accept replacement owner, destination or amount.
Repeated confirmation, even with a different operation ID, cannot mint again.
There is no currency conversion, fee, bank callback or real fiat settlement.

Routes: `POST /api/bramp/deposits`, `GET /api/bramp/deposits/{depositId}` and
`POST /api/admin/bramp/deposits/{depositId}/confirm`. Only the owner and system
operator may read the request; only the operator may confirm it. Mutations use
the signed operation/receipt/SSE pipeline and return HTTP 202.

## RAMP-002: Simulated withdrawal

The owner may request withdrawal of a strictly positive KVN amount only from
their free balance. The same atomic mock transaction places the amount in a
withdrawal hold; project escrow and other holds are unavailable. The request
remains Pending because no bank settlement exists. The owner or system
operator may cancel it once, returning the hold to free balance. A cancelled
request cannot be confirmed or cancelled again with a new operation ID.
Concurrent requests cannot reserve more than the free balance. No route
claims payment to a bank account.

Routes: `POST /api/bramp/withdrawals`,
`GET /api/bramp/withdrawals/{withdrawalId}` and
`POST /api/bramp/withdrawals/{withdrawalId}/cancel`. The cancellation actor is
derived from the verified origin, not supplied in the body. Exact signed
replays return the original receipt.

## COMP-001: Deliberate compatibility boundary

There is no standalone `pay`, `release`, `accept_and_pay`, payment refund or
generic payment dispute API. Token payments occur only through the approved
planning and milestone escrow transitions in SPEC-0003. Public project,
calendar and rating operations use existing typed `/api` routes rather than
legacy `/v1`, arbitrary contract-call wrappers or fake JSON-RPC. Document the
method mapping for the other frontend; add a route only when an existing
typed domain operation lacks a public equivalent. No mock state is copied to
the adapter. `POST /api/admin/fund` is available only in test/fixture mode and
is unavailable in a normal mock deployment.

## Security and verification

Use a reviewed WebAuthn implementation; do not write cryptographic verification
by hand. Apply the existing session, origin, CSRF, rate-limit and error policies.
Persist credential and challenge state in adapter PostgreSQL. Mock Bramp state,
holds, balances, nonces, receipts and events commit atomically under both memory
and SQLite storage. Test invalid origin, challenge, signature, UV, replay,
cross-account linking, concurrent deposit confirmation and withdrawal holds.
Neither Bramp nor passkey may release a disputed project's escrow.
