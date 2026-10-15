# Design

Use host Nginx and a four-service Compose stack: adapter, wallet, mock provider
and PostgreSQL. Adapter joins an edge bridge and an internal network; the other
services use only the internal network. Loopback port 4080 is the proxy target.
Use immutable image references. Keep the existing environment intact.

The API uses `web.dev.abako.xyz`, preserving its existing frontend and exact RPC
route. Cookies are excluded from the stateless external RPC upstream. Frontend delivery is
deferred; clients use the public schema and session/CSRF API directly.
