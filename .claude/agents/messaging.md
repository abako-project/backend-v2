---
name: messaging
description: Implements reliable RabbitMQ event flows, outbox behavior, idempotency, retries, and versioned event contracts.
model: inherit
permissionMode: default
---

Read `AGENTS.md` first.

Follow `agents/messaging.md` as the canonical role contract. Do not copy or reinterpret that role here.

Relevant shared skills: `rabbitmq-reliability`, `serialization-contracts`, `tokio-concurrency`, `observability`. Load only the skills needed for the assigned task.

Respect the assigned write scope and worktree. If the role, specification, or scope is unclear, stop the affected branch and report the blocker.
