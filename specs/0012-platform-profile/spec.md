# SPEC-0012: Public platform profile

Status: APPROVED
Owner: Digesta product owner
Created: 2026-10-21
Task: PLATFORM-001

## Problem and scope

The frontend needs deployment-specific branding and vocabulary for DAOs in
different domains. Add `GET /api/platform` with the object shape supplied by
the owner. One deployment serves one profile. The adapter owns this public
presentation configuration; it is independent of custody and provider state.

`requested-profile.json` records the owner's supplied profile with the current
approved product name. The approved runtime default uses `decimals: 0` for the current
mock integer units. Future chain integration may obtain actual asset metadata.

This feature does not create tenant isolation, DAO selection, governance,
dynamic administration, translations or new assets. It does not reinterpret
existing money, mutate provider state or change worker matching.

Branding: the owner confirmed Digesta as the default product name on 2026-10-27.

## Requirements

- PLATFORM-001: An anonymous GET returns 200 JSON with `id`, `name`, `tagline`,
  `description`, `sector`, `currency`, `projectTypes` and `terminology`.
  Nested property names follow the user's camelCase example. All listed
  terminology fields, including the external-profile hints, are present.
- PLATFORM-002: Ship the supplied software/Digesta profile as the default.
  Use currency decimals 0. Other fields retain the supplied values.
- PLATFORM-003: An optional `PLATFORM_CONFIG_PATH` selects an operator-owned
  JSON file. Load and validate it once at startup. Invalid or unreadable
  explicit configuration prevents startup; never silently use another DAO.
  An unset path selects the bundled default. Changes require restart.
- PLATFORM-004: The handler reads the validated profile in memory. It does not
  query PostgreSQL, custody, the mock, a bank or Kreivo. It needs no session or
  CSRF token. Existing Origin/CORS rules apply.
- PLATFORM-005: Define typed response DTOs and an OpenAPI response with an
  example. Treat all strings as plain text. Icon values are identifiers, not
  executable markup. Do not fetch profile URLs or inject HTML.
- PLATFORM-006: Bound the source file to 64 KiB, each string to 4,000 UTF-8
  bytes, and project types to 1–100 unique nonblank strings. Reject unknown
  properties, blank required strings, NUL and malformed nested values.
- PLATFORM-007: Existing public routes, stored records, sessions, amounts,
  signatures, operation tracking and SSE remain compatible.

## Approved compatibility decisions

The owner accepted configurable project types and authorized a sensible currency
scale on 2026-10-21. Use 0 for the current mock; no amount conversion is added.

- PLATFORM-008: Project briefs accept the exact configured project-type labels
  and all six legacy values: Other, SmartContract, Frontend, MVP, Audit and
  MobileApp. Preserve the exact submitted string on read. New unconfigured
  types are rejected. Old saved types remain readable after configuration
  changes; editing them requires a currently allowed or legacy value.
- PLATFORM-009: Currency metadata is deployment configuration, not a bank or
  chain connection. The default describes current mock units. A future provider
  can derive metadata from pallet-assets; this release does not query a chain.

## Verification and delivery

Exercise anonymous GET, the complete JSON shape, configured alternate DAO,
existing Origin rules, startup validation and unchanged amount serialization.
Verify custom-type writes, legacy values and reads of previously saved briefs.
Run applicable repository gates and record evidence.
No database migration or dependency is needed.
Publishing images and deploying the feature are separate release actions.

## Approval

Approved by the product owner on 2026-10-21: requested the public profile,
authorized a sensible decimal value and accepted configured brief types while
preserving existing values. Implementation scope follows the resolved draft.
