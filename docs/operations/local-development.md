# Local Development

Use the [local deployment guide](../../infra/README.md) for the complete startup, generated-secret and independent-frontend instructions. Run its commands from the repository root. RTK is an optional agent tool, not a prerequisite for users or deployment.

The stack contains Nginx, independent Leptos, adapter-api, wallet and mock-provider. The gateway listens on localhost:8088. Docker builds the images; native Rust commands use rust-toolchain.toml. PostgreSQL and RabbitMQ are not required.

Each backend owns a separate SQLite directory. Adapter/custody migrations run at startup; the mock initializes its own state table and optional catalogue seed. Keep generated secrets outside the repository and preserve the same secret/data set when restarting.

```sh
docker compose -f infra/compose.yaml ps
docker compose -f infra/compose.yaml logs --tail=100
docker compose -f infra/compose.yaml down
```

Keep the environment variables from startup in the same terminal. Stopping Compose does not delete bind-mounted data. A directory under /tmp may be removed by the operating system.

Verification entry points are [scripts/verify.sh](../../scripts/verify.sh), [scripts/poc-e2e.py](../../scripts/poc-e2e.py) and [infra/verify.py](../../infra/verify.py). The API E2E uses real local services; the infrastructure smoke checks Compose/Nginx boundaries.
