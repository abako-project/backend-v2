# PLATFORM-001 tasks

Root is the sole primary writer/integrator. No agent delegation is assigned.

| Task | Scope | Dependency | Completion |
|---|---|---|---|
| PLATFORM-01 | This specification directory | None | Resolve scale and brief-type questions; approved spec |
| PLATFORM-02 | `crates/generated-contracts/src/platform.rs`, crate export, `services/adapter-api/src/platform/**`, adapter module/state/router wiring | PLATFORM-01 | Typed bounded configuration and public GET |
| PLATFORM-03 | `contracts/openapi.json`, `crates/generated-contracts/API.md`, public OpenAPI contract tests, adapter README and relevant configuration docs | PLATFORM-02 | Frontend contract and operator instructions match implementation |
| PLATFORM-04 | Focused adapter/contract tests, `scripts/poc-e2e.py` and `progress/handoffs/PLATFORM-001.md` | PLATFORM-02, PLATFORM-03 | Applicable gates and regression evidence |

PLATFORM-02 also owns the adapter
project-brief models/validation/tests and their OpenAPI schemas. No unrelated
profile, provider, custody, money or frontend behavior is in scope.

The E2E scope includes the public profile check and correcting the stale
single-milestone coordinator-capacity expectation from 160 to its declared
60 minutes, consistent with approved SPEC-0003's removed planning reservation.
The existing profile round-trip fixture also includes the current nullable
contactEmail field so exact comparison matches the implemented profile DTO.

Implementation is approved. New custom brief values remain readable by this
release; rolling back to an older enum-only binary after saving custom types
requires a forward fix or reviewed data compatibility handling. Metadata alone
changes no stored state.

After verification, the integrator may fast-forward the clean local master to
the feature commit. This does not authorize image publication or deployment.
