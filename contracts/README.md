# Contract Governance

- Browser clients use the same REST/JSON and SSE contracts; GraphQL is not part of this project.
- OpenAPI documents HTTP requests, responses, errors, authentication, and asynchronous operation tracking.
- Internal signed provider messages are versioned separately from HTTP route paths.
- Shared Rust wire types must not expose service database rows or require a frontend to depend on a backend service crate.

Every contract has an owner, compatibility policy, generation procedure, and breaking-change approval path.
