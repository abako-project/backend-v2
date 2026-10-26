# Local development

Use the [Compose startup guide](../../infra/README.md#start) from the repository
root. It covers prerequisites, generated secrets, administrator login, data
persistence and the independent frontend. No agent setup is required.

The stack includes Nginx, a frontend, adapter-api, PostgreSQL, wallet and
mock-provider. The gateway listens on localhost:8088. The adapter stores its data
in PostgreSQL; custody and the default mock use separate SQLite stores. Migrations
run at startup, and the mock initializes missing catalog entries. Keep the same
secret/data set when restarting. See [data ownership](../architecture.md).

```sh
docker compose -f infra/compose.yaml ps
docker compose -f infra/compose.yaml logs --tail=100
docker compose -f infra/compose.yaml down
```

Keep the startup environment variables in the same terminal. Stopping Compose
preserves its PostgreSQL volume and bind-mounted custody/mock data. `/tmp` may be
cleared by the operating system; use a private persistent directory for data you
need to retain. Follow the startup guide to reset a coherent environment rather
than deleting individual service stores.

## Verification

See [verification and CI](../../ci/README.md). Rust and API E2E checks are separate
from the Compose/Nginx smoke in `infra/verify.py`. API E2E starts disposable
PostgreSQL and real services; it does not run a browser.

## Local development accounts


These disposable accounts are for the local mock backend only. Each starts with
1000 simulated KVN. Workers have 40 hours/week, all seeded skills and all
non-coordinator roles so the complete development flow can be exercised.
Coordinators additionally have server-granted eligibility and Coordinator mode.

| View | Username | Display name | Password |
| --- | --- | --- | --- |
| Worker | `worker1` | Worker 1 | `Worker 1 1234!` |
| Worker | `worker2` | Worker 2 | `Worker 2 1234!` |
| Worker | `worker3` | Worker 3 | `Worker 3 1234!` |
| Worker | `worker4` | Worker 4 | `Worker 4 1234!` |
| Worker | `worker5` | Worker 5 | `Worker 5 1234!` |
| Worker | `worker6` | Worker 6 | `Worker 6 1234!` |
| Worker | `worker7` | Worker 7 | `Worker 7 1234!` |
| Worker | `worker8` | Worker 8 | `Worker 8 1234!` |
| Worker | `worker9` | Worker 9 | `Worker 9 1234!` |
| Worker | `worker10` | Worker 10 | `Worker 10 1234!` |
| Coordinator | `coordinator1` | Coordinator 1 | `Coordinator 1 1234!` |
| Coordinator | `coordinator2` | Coordinator 2 | `Coordinator 2 1234!` |
| Coordinator | `coordinator3` | Coordinator 3 | `Coordinator 3 1234!` |
| Client | `client1` | Client 1 | `Client 1 1234!` |
| Client | `client2` | Client 2 | `Client 2 1234!` |
| Client | `client3` | Client 3 | `Client 3 1234!` |

Create/check these accounts through the API after starting the local stack:

```sh
python3 scripts/seed-dev-users.py --base-url http://localhost:8088 \
  --admin-password-file "$DIGESTA_LOCAL_DIR/secrets/bootstrap-admin-password"
```

The script reuses existing actors, verifies their profiles and coordinator modes,
and tops up balances below 1000 KVN through simulated Bramp deposits. It never
resets the databases. Development passwords above are not production credentials.

The API seed also saves private contact email for clients/workers and descriptive experience for each worker role in adapter/PostgreSQL. Experience does not change provider matching. Keep `DIGESTA_LOCAL_DIR` in persistent storage outside `/tmp`. Existing environments must retain their actual secrets directory and database settings; see [rename migration notes](../project/digesta-rename.md).
