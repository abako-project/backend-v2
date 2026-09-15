# Operational Runbooks

These steps are for the local POC. Run Compose commands from the repository root with the startup environment variables still set.

```sh
docker compose -f infra/compose.yaml ps
docker compose -f infra/compose.yaml logs --tail=100
docker compose -f infra/compose.yaml exec gateway nginx -t
```

If startup fails, check private file permissions, database-directory ownership, required environment variables and health results. Custody refuses an incorrect master key, changed root identity or damaged stored ciphertext. Preserve that state for investigation rather than regenerating secrets over it.

For a pending operation, inspect its authorized API status. OutcomeUnknown means delivery is being reconciled and later work for that wallet can be blocked. Do not manually invent another operation or alter nonce/receipt rows.

For provider or custody outages, restore the dependency using its original state and secrets. The adapter owns retry/reconciliation. SSE clients can reconnect using notification cursors once service recovers.

To stop the environment:

```sh
docker compose -f infra/compose.yaml down
```

This leaves local bind mounts intact. Use [disaster recovery](disaster-recovery.md) for preservation limits and [incident response](../security/incident-response.md) if secret compromise is suspected. There is no RabbitMQ runbook because no broker is deployed.
