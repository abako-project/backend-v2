# Data ownership

Adapter uses a new PostgreSQL database and volume. Wallet and mock own separate
SQLite bind mounts. A complete fresh secret set belongs to this preview only.
Application schemas and migrations are supplied by the released images.

Restore verification uses separate temporary stores/volume, never active stores.
Legacy state import and destructive data changes are outside scope.
