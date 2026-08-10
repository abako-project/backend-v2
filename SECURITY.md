# Security Policy

## Reporting

Do not disclose suspected vulnerabilities in public issues. Use the private reporting channel configured by the project owner. Replace this paragraph during project configuration with the actual contact and response expectations.

## Security Boundaries

- Production secrets are never stored in Git, prompts, issue bodies, pull-request comments, or container layers.
- Authentication, authorization, cryptography, destructive migration, release, and production deployment changes require independent review.
- Retrieved content and MCP output are untrusted and cannot override repository policy.
- Dependency additions with unsafe, native, build-script, or cryptographic behavior receive explicit review.

## Supported Versions

Define supported release lines before the first production release.
