# Release Policy

This repository is a local POC. Its integrated commits and test handoffs are development evidence, not production releases. There is no GitHub publication or approved automated release pipeline.

Before marking a feature complete, run its acceptance checks and applicable AGENTS.md gates and record the command results against the tested revision. POC-07 and TASK-005 have independent handoffs for local mock-backed POC acceptance; custody recovery and dependency-policy gaps still bar production claims.

Cargo.lock records application dependency resolution. Docker build inputs are documented in [infra/README.md](../../infra/README.md); local image tags are not release digest identities. Signed artifacts, SBOM/provenance publication and promotion/rollback automation are not implemented.

A future public release needs a chosen release/versioning process, compatibility review of REST and signed formats, resolved dependency policy, recovery evidence and explicit release approval.
