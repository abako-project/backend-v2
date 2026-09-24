# Observability

The POC provides internal health/readiness probes, durable audit records and custody queue metrics. It does not deploy Prometheus, Grafana, an OpenTelemetry collector or production alerting.

The custody bearer-protected /internal/metrics endpoint reports queue/outcome/attempt/age/lease-recovery counters. Adapter transport operations and provider receipts expose progress through authorized APIs. Correlate failures using operation IDs and provider instance IDs, not secret payloads.

Nginx access logs omit query strings and bodies. Inspect local container status and bounded logs with:

```sh
docker compose -f infra/compose.yaml ps
docker compose -f infra/compose.yaml logs --tail=100
```

Never enable logging of seeds, master keys, service credentials, passwords, cookies or arbitrary signing payloads. The local E2E runner scans bounded service logs, API responses, SSE and custody metrics for known secret markers. That and selected negative tests do not prove all outputs are secret-free; TASK-005 still needs independent review.

No service-level objectives, paging ownership, dashboards or production capacity claims have been established.
