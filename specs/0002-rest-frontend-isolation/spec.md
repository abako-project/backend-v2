# SPEC-0002: REST and frontend isolation

Status: APPROVED

Owner: Kunveno product owner

## Requirements

- REST-001: Expose one documented REST/JSON browser API under `/api`, without an obligatory `/v1` path segment. Version signed provider formats independently.
- REST-002: Both the team's external frontend and the Leptos application consume that API. Backend crates do not depend on the frontend; the frontend does not import backend service implementations or database models.
- REST-003: Document authentication, credentials, CSRF, origin policy, typed errors, idempotency, operation status, and SSE resume in OpenAPI and integration examples before frontend handoff.
- REST-004: A state-changing provider action returns an operation reference. A transport acknowledgement is not business completion. Clients can query the outcome without retaining an SSE connection.
- REST-005: Configure an exact allowed-origin list. Never combine wildcard origins with credentialed requests. Authenticate event streams and notification reads from the session, not from a caller-supplied recipient.
- REST-006: Persist notification delivery data and explicit read state. Resume after a cursor. Receiving or replaying an event does not mark it read.
- REST-007: Build and serve Leptos independently. Configure its API location without coupling backend code to a particular frontend origin.
- REST-008: Compose may provide explicit Nginx routing to adapter and frontend containers. Disable SSE buffering and set stream-appropriate timeouts. Do not publish custody or mock internal ports through Nginx.

## Non-goals

Leptos server-function APIs, production deployment, Kubernetes, automatic proxy discovery, and pricing-policy decisions.

## Verification

Exercise the same authenticated API from two allowed origins. Reject a disallowed origin and forged mutation. Reconnect SSE with a cursor and prove unread state is unchanged. Build the backend without frontend tooling and the browser app without backend service dependencies. Validate Compose and Nginx configuration and run an HTTP smoke test through the proxy.
