# Messaging Architecture

The POC uses private HTTP requests and durable SQLite state. There is no RabbitMQ broker, webhook receiver, exchange, consumer acknowledgement or dead-letter queue in the deployed stack.

The adapter records a domain operation, obtains its signature from custody, then submits the exact signed SCALE envelope to the mock. Operations are ordered per wallet. An ambiguous response becomes OutcomeUnknown and is reconciled against the existing receipt; it must not create a replacement business action.

The mock commits state, account nonce, receipt and provider events atomically. Exact retries are idempotent; conflicting reuse of an operation identifier is rejected. Provider instance identity prevents old signed operations being retargeted after a reset.

An ingestion task inside adapter-api polls provider events, validates cursor progression and commits recipient notifications with the ingestion cursor. It is not a separate deployed event-ingestor service. Dependency failures back off up to 30 seconds.

The signed envelope is versioned independently of the REST URL. Public contracts and replay rules are documented in [API.md](../../crates/generated-contracts/API.md). Introducing a broker or external webhook requires a new approved delivery contract.
