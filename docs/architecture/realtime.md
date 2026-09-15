# Realtime Architecture

The browser receives one-way notifications over authenticated SSE, as required by [SPEC-0002](../../specs/0002-rest-frontend-isolation/spec.md). There is no WebSocket protocol in the POC.

The session determines the recipient; callers cannot select another account. Active streams revalidate session expiry/revocation. The stream sends notification IDs for resume and a keepalive every 15 seconds. Notification receipt or replay never marks it read; an explicit authorized REST mutation does that.

Notifications persist in adapter SQLite. The provider's event cursor is separate from the adapter's notification ID. Reconnect using the documented cursor contract in [API.md](../../crates/generated-contracts/API.md); operation status remains queryable without SSE.

Delivery reads pages of 100, uses a 32-item channel and closes delivery if a send waits five seconds. Connection admission is bounded in the adapter. These limits protect transport, not worker matching.

Nginx disables SSE buffering and caching and uses a one-hour read timeout. The current process polls notification storage; there is no distributed fan-out infrastructure or established production connection-capacity target.
