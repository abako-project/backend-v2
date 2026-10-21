# Security boundaries

The corrected host uses its existing valid HTTPS certificate. Keep secure,
HttpOnly, SameSite=Lax cookies and exact-origin/CSRF checks. Sessions remain
host-scoped; do not add a parent-domain cookie or wildcard CORS. WebAuthn
credentials tied to the previous RP may need enrollment on the new RP.
Password credentials and account data remain available.

Backups may contain sensitive Nginx settings and stay private on the server.
Never print raw configuration, Docker environments, cookies or credentials.
