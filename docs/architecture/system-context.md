# System Context

The local Rust POC implements the approved marketplace workflows in [SPEC-0003](../../specs/0003-transactional-marketplace/spec.md). Clients request work, assigned coordinators prepare plans, workers receive assignments, and administrators manage privileged catalogue and coordinator actions.

```text
Browser (Leptos or external frontend)
  -> Nginx -> adapter-api -> wallet (signing)
                         -> mock-provider (business execution)
```

The adapter exposes REST/JSON under /api and authenticated SSE. Leptos is an independent browser build. Custody and provider listeners remain private. See [ADR-0001](adr/0001-rest-and-independent-frontends.md).

The mock is the business source of truth: worker profiles, calendars, proposals, milestone task-storage instances, assignments, reputation and simulated funds. Its contract instances are modules/data inside one atomic runtime, not separate deployed services.

The adapter owns classic login, transport operations and notification read state. Custody owns encrypted signing seeds. The current stack uses no blockchain, HSM, PostgreSQL, RabbitMQ or Kubernetes. A future chain provider needs a reviewed transport/encoding integration; changing a URL alone does not establish blockchain compatibility.
