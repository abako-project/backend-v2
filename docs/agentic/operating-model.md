# Agentic Operating Model

## Workflow Graph

```text
Human objective
  -> project discovery
  -> specification
  -> specification review
  -> human approval
  -> architecture and contracts
  -> isolated implementation tasks
  -> independent verification
  -> independent review
  -> integration
  -> protected CI
  -> approved release and deployment
```

The graph may contain bounded implementation-verification loops. Every loop has an iteration budget, failure classification, external verifier, and stop condition.

## Parallelism

Parallelize only tasks with independent inputs and non-overlapping write scopes. Contracts and migrations normally precede dependent implementation. One integration role owns shared manifests, lockfiles, generated code, and merge order.

## Persistent State

Specifications, status files, task graphs, ADRs, contracts, handoffs, CI artifacts, and deployment records are persistent state. Conversation history is not a substitute.

## Human Gates

Human approval is mandatory for specification approval, destructive data changes, breaking contracts, authentication or authorization policy, release publication, production deployment, and accepted security risk.
