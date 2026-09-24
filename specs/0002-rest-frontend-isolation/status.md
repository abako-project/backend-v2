# Specification status

State: APPROVED

Approved: 2026-09-08 by the product owner in conversation.

Scope: REST/JSON shared browser contract, independently built frontends, isolated backend services, and optional local Nginx/Compose routing.

Last updated: 2026-09-24.

Implementation: Integrated at `3ab2b58` on `feat/rust-rest-poc`.

Recorded verification: typed REST/OpenAPI contract tests; adapter authentication,
CORS/CSRF and SSE tests; browser fixture smoke; real-service memory/SQLite flows;
independent frontend image build; healthy five-service Compose stack and Chromium
mount through the gateway. Nginx checks cover public routing, credential headers,
SSE cursor handling and denial of internal routes.

Evidence: `../../progress/handoffs/REST-POC-foundation.md`,
`../../progress/handoffs/POC-01B.md`, `../../progress/handoffs/POC05.md`, and
`../../progress/handoffs/POC07-frontend-build.md`.

Closure: POC-07's final independent backend acceptance review is complete for
the local mock-backed POC; see `../../progress/handoffs/POC-07-verification.md`.
Its task is distinct from the similarly named frontend-build handoff. The workspace
dependency-policy gate remains outside this backend closeout: cargo-deny 0.20.2 returned exit 5
for license allowances and two unmaintained Leptos dependencies. No exceptions
were added. See `../../docs/dependencies/poc-audit-2026-09-10.md`.

Related work: Custody is specified in SPEC-0001; the approved planning-price
negotiation is specified in SPEC-0003. Neither approval implies production readiness.

Human deployment and verification commands use native tools.
The real-service E2E passed again for both backends on 2026-09-23. It now runs
single-milestone, four-milestone and dispute API scenarios, separate from the
browser fixture smoke. See `../../progress/handoffs/DSP-07.md`.
The owner accepted maintenance-advisory deferral; applying that policy is pending.
