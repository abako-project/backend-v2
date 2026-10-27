# provider

Shared server support for the native and WASM providers. `execution` verifies
signatures and handles generation, nonces, replay, receipts and event ordering.
`api` defines the provider interface; `http` serves it with service authentication.
Each implementation owns business execution and atomic persistence.

Start with [src/lib.rs](src/lib.rs); see [execution boundaries](../../docs/architecture.md#execution).
