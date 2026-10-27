# provider-execution

Shared signed-operation handling for the native and WASM providers: signature
verification, generation/nonce checks, exact replay, receipts and event ordering.
The provider supplies business outcomes and atomically persists the resulting state.
This crate owns neither signing keys nor marketplace rules.

Start with [src/lib.rs](src/lib.rs); see [execution boundaries](../../docs/architecture.md#execution).
