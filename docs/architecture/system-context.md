# System context

Clients request work, coordinators prepare proposals and oversee delivery, workers
receive assignments, and administrators manage privileged catalog and coordinator
actions. The public API serves independent browser frontends. See the
[current implementation](../project/project-context.md) and [ADR-0001](adr/0001-rest-and-independent-frontends.md).

```text
Browser (Leptos or external frontend)
  -> Nginx -> adapter-api -> wallet (signing; custody SQLite)
                         -> mock-provider (business execution; SQLite or memory)
                         -> PostgreSQL (adapter data)
```

The adapter exposes REST/JSON under `/api` and authenticated SSE. It owns identity,
sessions, descriptive records, transport operations and notification read state
in PostgreSQL. Custody owns encrypted signing seeds and durable signing jobs in
SQLite. Neither wallet nor provider is browser-accessible.

The mock owns qualifications, calendars, proposals, milestone task storages,
assignments, reputation, simulated funds, receipts and events. Its contract
instances are domain objects inside one transactional Rust runtime. The SQLite
backend persists that runtime as one serialized state row; memory mode loses it
on restart. See [data ownership](data-ownership.md).

The current backend has no blockchain execution, contract WASM runtime, HSM,
RabbitMQ or Kubernetes dependency. A future Kreivo provider requires compatible
call encoding, submission and receipt/event reconciliation behind the browser
API. Running the ink! v5 contracts inside the mock remains future work.
