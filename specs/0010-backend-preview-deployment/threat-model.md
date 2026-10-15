# Security boundaries

Terminate HTTPS at host Nginx. Keep custody, mock and PostgreSQL unexposed.
Retain exact-origin, CSRF and secure-cookie checks. New public access does not
grant administrator authority. Bootstrap and service credentials remain in
private server files; commands and test output exclude their values.

Backups contain encrypted custody data and matching secrets and require private
storage permissions. The mock is test data; real funds and production key
management remain outside the approved development scope.
