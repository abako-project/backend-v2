# Database schemas

The API owns PostgreSQL tables initialized by `services/api/migrations`.
The wallet owns SQLite migrations in `services/wallet/migrations`.
The contract provider owns its SQLite checkpoint schema; runtime state and the
operation shell commit together. There are no cross-service foreign keys.

Fresh development installations initialize matching API, wallet and provider
state. Existing incompatible stores fail startup; no historical converter or
silent reset is provided. Backups must preserve all three stores and their secrets;
see [recovery](../docs/operations/disaster-recovery.md).
