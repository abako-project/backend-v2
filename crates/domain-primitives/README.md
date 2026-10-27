# domain-primitives

Shared value types: account/entity IDs, exact integer money, minutes and ISO weeks.
Constructors and JSON/SCALE decoding enforce their invariants. Used by backend
services and the browser; contains no storage or marketplace workflow.

Start with [src/lib.rs](src/lib.rs); see the [wire types](../generated-contracts/README.md).
