# ADR-0001: REST and independent frontends

Status: ACCEPTED

Date: 2026-09-08

Authority: The product owner accepted the architecture review, explicitly rejected GraphQL, and requested independent external and Leptos frontends.

## Decision

Use a documented REST/JSON API and authenticated SSE. Both frontends consume exactly the same public contract. The adapter neither serves frontend assets nor requires Leptos server functions. Leptos is a separately built browser application.

Deploy adapter, custody, mock provider, and frontend as separate processes/containers. Keep simulated business contracts as separate modules and instances inside one transactional mock runtime. Contract instances are not deployment units. The mock owns business truth; custody owns keys; the adapter owns credentials, sessions, pending transport operations, and notification read state.

For local Compose, an explicit Nginx configuration routes `/api/` to the adapter and frontend traffic to a separate frontend container. SSE proxy buffering is disabled. An optional external frontend can replace the frontend upstream without rebuilding the backend. Direct frontend-to-API access uses an explicit CORS origin allowlist and the documented credential/CSRF contract. No Docker socket or automatic nginx-proxy discovery is needed.

## Consequences

The other frontend team can integrate from OpenAPI and examples without Rust or Leptos. Browser API changes require contract updates and compatibility review. No browser may access custody or provider-internal endpoints. Replacing the mock later requires a new provider implementation, not a change to the browser framework.

No pricing choice is made by this ADR. Planning fees and milestone payouts require their own approved business rules.
