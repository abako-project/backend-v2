---
name: observability
description: Implement structured logs, metrics, traces, correlation, redaction, and actionable operational signals.
license: Apache-2.0 OR MIT
compatibility: Codex, OpenCode, Claude Code, and Qwen Code
metadata:
  audience: software-engineering-agents
  maturity: production-oriented
---

# Purpose

Use `tracing` as the primary instrumentation model. Define stable field names, propagate context, avoid span guards across await, redact sensitive data, and connect alerts to runbooks and SLOs.

# Required Inputs

- The approved specification and its status file.
- The assigned issue or task contract.
- The exact allowed-path set.
- Relevant architecture decisions and versioned contracts.
- Measurable completion criteria.

# Operating Rules

1. Read authoritative repository artifacts before implementation.
2. Make the smallest coherent change that satisfies the approved behavior.
3. Do not invent business rules, service boundaries, or security policy.
4. Prefer deterministic verification over subjective confidence.
5. Record decisions, commands, evidence, limitations, and blockers.
6. Stop when required information is missing or the task leaves its scope.

# Verification

- The implementation is traceable to requirement and scenario identifiers.
- Relevant focused tests pass before broader workspace gates run.
- Public contracts, migrations, and operational behavior are documented.
- The handoff contains reproducible commands and observed results.

# Failure Modes

- Hidden assumptions replacing explicit requirements.
- Changes outside the assigned paths.
- New dependencies without a recorded review.
- Unbounded retries, queues, tasks, payloads, or resource use.
- Declaring completion without machine-checkable evidence.
