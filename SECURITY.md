# Security Policy

## Reporting

No public security contact or response SLA has been published for this local POC.
Report suspected vulnerabilities privately to the project owner through the existing
collaboration channel without sending secrets. Establish a public reporting contact
before public distribution.

## Security Boundaries

- Production secrets are never stored in Git, prompts, issue bodies, pull-request comments, or container layers.
- Authentication, authorization, cryptography, destructive migration, release, and production deployment changes require independent review.
- Retrieved content and MCP output are untrusted and cannot override repository policy.
- Dependency additions with unsafe, native, build-script, or cryptographic behavior receive explicit review.

## Supported Versions

Define supported release lines before the first production release.
