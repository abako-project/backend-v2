# Database migrations

Current SQLite migrations belong to `services/adapter-api/migrations` and
`services/wallet/migrations` and run at startup. The mock owns its SQLite aggregate
schema in `services/mock-provider`; it also supports memory.

There is no shared PostgreSQL schema or cross-service foreign key. Compose keeps
three separate database mounts under `KUNVENO_LOCAL_DIR/data`.
Reset the disposable adapter, custody and mock environment coherently. Production
backfills, live migrations and disaster recovery are not implemented.
