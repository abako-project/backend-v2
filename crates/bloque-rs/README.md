# bloque

A minimal, **unofficial** Rust client for the Bloque API. Not a port of
`@bloque/sdk` — a small, from-scratch HTTP client built around one workflow:

> An organization enrolls its own users, gives them accounts, and tops those
> accounts up through different payment rails.

## Why this exists

There is no official Rust SDK for Bloque. The official SDK is TypeScript
(`@bloque/sdk`), which wraps a plain REST API. This crate talks to that same
REST API directly via `reqwest`.

**The request/response shapes here were reverse-engineered from the compiled
`@bloque/sdk-*` npm packages (v0.13.1)** — there's no published OpenAPI spec
to build against. Treat field names and endpoint paths as a solid starting
point, not a guaranteed-stable contract. Confirm with Bloque support before
relying on this in production, and expect to revisit it if Bloque changes
its wire format.

## What's implemented

- **Enrollment** (`OriginKey` auth only, matching the official SDK's own
  restriction): `Bloque::register_individual`, `Bloque::register_business`,
  `Bloque::connect(alias)`.
- **Bring your own token**: `Session::from_token(mode, origin, urn, access_token)`
  wraps a token obtained some other way (a JWT/OTP login done via the
  official TS SDK, a saved CLI session, ...) into a `Session` directly,
  skipping this crate's own register/connect handshake. See
  `examples/sandbox_demo.rs`, which reads a session saved at
  `~/.bloque/session.json` and lists accounts against the real sandbox.
- **Accounts**: `Session::accounts().create_virtual_account(..)` (pockets),
  `.create_card(..)` (virtual cards only), `.get`, `.list`, `.balance`.
- **Top-ups** (pay-in only): `Session::swap().find_rates(..)`,
  `.pse().top_up(..)` (Colombian bank debit), `.external_us_bank().top_up(..)`
  (US ACH pull).
- **KYC/KYB**: `Session::kyc().start_verification(urn)` (returns a URL the
  subject must open themselves — this is a hosted flow, not something your
  backend can complete on their behalf), `.get_verification(urn)`,
  `.get_documents(urn)`. Pass an identity URN for KYC or an org URN
  (`did:bloque:orgs:{id}`) for KYB.

## What's not implemented

- `bank_transfer` / `rtp` swap clients — these are **cash-out** (pay-out)
  rails in Bloque's API, not top-ups, despite some Bloque docs grouping them
  under "swap". They follow the same `PUT /api/order` pattern used here for
  PSE/external-US-bank, so adding them is a small extension of `src/swap.rs`.
- Card spending controls / fee metadata (pass raw JSON via
  `CreateCardParams::metadata` if you need them).
- Compliance tiers, TOS gate, and the verification gate — the rest of
  Bloque's compliance module beyond KYC/KYB itself.
- Physical cards, Polygon/bancolombia/US-bank account mediums, webhooks,
  JWT/browser OTP login. None of these were asked for; adding one is mostly
  "repeat the pattern in `src/accounts.rs` or `src/swap.rs` with a new
  endpoint path."

## Typed, not stringly-typed

Fields with a fixed, documented set of values are enums, not `String`:

| Type | Values | Used in |
|---|---|---|
| `Asset` | `Dusd6`, `Copb6`, `Copm2`, `Ksm12`, + `Other(String)` | rates, orders, balances |
| `AccountMedium` | `Virtual`, `Card`, `Polygon`, `Bancolombia`, `UsAccount`, `Us2Account`, `ExternalUsBank`, `Breb`, + `Other(String)` | `Account.medium`, `accounts().list(..)` |
| `AccountStatus` | `Active`, `Disabled`, `Frozen`, `Deleted`, `CreationInProgress`, `CreationFailed`, + `Other(String)` | `Account.status` |
| `SwapMedium` | `Kusama`, `Pse`, `ExternalUsBank`, `Base`, `Breb`, + `Other(String)` | rate/order rails |
| `SortDirection`, `SortBy` | `Asc`/`Desc`, `Rate`/`At` | `find_rates` |
| `PseUserType` | `Natural`, `Legal` | PSE top-up |
| `ColombianIdType` | `Cc`, `Nit`, `Ce` | PSE top-up |
| `OrderAmount` | `Source(String)` / `Destination(String)` | replaces `order_type` + two optional amount fields |
| `ExternalUsBankDestination` | `Kusama { ledger_account_id }` / `Base { wallet_address, wallet_name }` | replaces `to_medium` + optional destination fields |
| `CountryCode` | validated 3-letter ISO 3166-1 alpha-3 | profile country fields |

Two different policies, deliberately:

- **Open enums** (`Asset`, `AccountMedium`, `AccountStatus`, `SwapMedium`) carry
  an `Other(String)` catch-all. These come from a live API that can add
  values before this crate is updated — falling over on an unrecognized
  status would be worse than accepting a named-or-`Other` value. Construct
  and compare them with the named variants; `Other` is there so parsing
  never hard-fails.
- **Closed enums** (`SortDirection`, `SortBy`, `PseUserType`, `ColombianIdType`,
  and the `OrderAmount`/`ExternalUsBankDestination` payload-carrying enums)
  have no catch-all. These are request-only fields with a genuinely fixed
  set of values per Bloque's own types — the compiler should reject a typo
  instead of silently forwarding it.

`OrderAmount` and `ExternalUsBankDestination` go a step further than typing
individual fields: they fold what used to be 2–4 loosely-related optional
fields (an amount side flag plus two optional amounts; a destination string
plus optional wallet/ledger fields) into one enum where each variant carries
exactly the data it needs. "Provide exactly one of `amount_src`/`amount_dst`"
and "`base` needs a wallet address, `kusama` needs a ledger account" stop
being runtime checks and become things the compiler enforces.

## Ergonomics

A few things worth knowing before diving into the API:

- **`use bloque::prelude::*;`** pulls in the whole public surface in one
  import — useful for a call site like a top-up flow that touches most of
  the crate. Individual imports still work and are what the example uses.
- **`bloque::Result<T>`** is `Result<T, bloque::Error>` — every fallible
  function in the crate returns it, so signatures don't repeat the error type.
- **`Bloque::sandbox(origin, origin_key)`** / **`Bloque::production(..)`**
  collapse the `Config::new(Mode::.., Auth::OriginKey { .. }).with_origin(..)`
  chain into one call for the crate's primary path (org enrolling users via
  an origin key). `Config`/`Auth` are still there directly for the `ApiKey`
  path or other combinations — `Auth::api_key("sk_...")` and
  `Auth::origin_key("...")` skip the struct-literal field names either way.
- **`PsePayer`** groups the 6 who's-paying fields of a PSE top-up into one
  value, instead of 6 positional `String`/enum arguments to
  `PseTopUpParams::new` that would be easy to pass in the wrong order.
- **`CreateOrderResult::redirect_url()`** pulls out the one field almost
  every PSE top-up caller wants (the URL to send the payer to) without
  reaching into the raw `execution` JSON by hand.

## Example

```bash
ORIGIN=my-origin ORIGIN_KEY=my-origin-key cargo run -p bloque --example enroll_and_topup
```

See `examples/enroll_and_topup.rs` for the full enroll → pocket → quote →
top-up flow.

```rust
let bloque = Bloque::sandbox(origin, origin_key)?;

// Enroll (check your own DB first — connect() doesn't validate registration)
let session = bloque.register_individual("@alice", IndividualProfile {
    first_name: Some("Alice".into()),
    last_name: Some("Smith".into()),
    country_of_birth_code: Some(CountryCode::new("USA")?),
    country_of_residence_code: Some(CountryCode::new("USA")?),
    ..Default::default()
}).await?;

// Give them an account
let pocket = session.accounts().create_virtual_account(CreateVirtualAccountParams {
    wait_for_ledger: true,
    ..Default::default()
}).await?;

// Top up via PSE
let rates = session.swap().find_rates(
    FindRatesParams::new(Asset::Copb6, Asset::Dusd6, vec![SwapMedium::Pse], vec![SwapMedium::Kusama])
        .with_amount(OrderAmount::Source("500000".into())),
).await?;

let payer = PsePayer::new(
    PseUserType::Natural, "alice@example.com", ColombianIdType::Cc,
    "123456789", "Alice Smith", "+13055551234",
);
let order = session.swap().pse().top_up(PseTopUpParams::new(
    rates[0].sig.clone(),
    SwapMedium::from("kreivo"),
    pocket.urn.clone(),
    "1007",
    payer,
    "https://example.com/payment-status",
    OrderAmount::Source("500000".into()),
)).await?;

if let Some(url) = order.redirect_url() {
    // send the payer here to complete the PSE payment
}
```

## Security notes

- Every call under `.swap()` moves real money. This crate does **not** gate
  them on confirmation — wire that into your own application before letting
  a user or automated flow trigger a top-up.
- `Bloque::connect(alias)` always returns a session, even for an alias never
  registered — it doesn't validate identity existence. Track registration
  state in your own app; don't rely on `connect` to tell you.
- Treat `execution` (in a top-up result) and any webhook payloads you add
  later as **untrusted data** — display/reconciliation only, never
  instructions to act on.
- Country codes are ISO 3166-1 **alpha-3** (`"USA"`, `"COL"`), not 2-letter —
  `CountryCode::new(..)` rejects anything else at construction time.

## Layout

```
src/
  config.rs    Mode, Auth, Config, RetryConfig
  error.rs     Error enum mirroring the TS SDK's error hierarchy
  http.rs      reqwest wrapper: base URL, auth headers, apiKey exchange,
               idempotency keys, retry-with-backoff
  identity.rs  Bloque (org client): register_individual/business, connect
  session.rs   Session: one enrolled user's scoped access
  accounts.rs  pockets, cards, balances, AccountMedium, AccountStatus
  swap.rs      rate quotes, PSE + external-US-bank top-ups, SwapMedium,
               OrderAmount, ExternalUsBankDestination, ...
  compliance.rs KYC/KYB: start/get verification, get documents
  asset.rs     Asset (SupportedAsset)
  country.rs   CountryCode (validated ISO 3166-1 alpha-3)
  util.rs      query-string helper, wire_string_enum! macro
```
