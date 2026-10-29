# protocol

Shared value types and messages for the browser, API, wallet and providers.
`values` validates IDs, exact money, minutes and weeks; the other modules define
REST data, signed commands, receipts and core views with JSON/SCALE encoding.
Contains no service implementation, signing keys or marketplace execution rules.

Start with [src/lib.rs](src/lib.rs) and [values](src/values/mod.rs).
See the [API guide](API.md) and [OpenAPI contract](../../contracts/openapi.json).
