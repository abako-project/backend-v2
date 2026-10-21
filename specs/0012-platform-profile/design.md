# Design

The adapter loads a typed `PlatformProfile` before admitting requests and
serves it through `GET /api/platform`. Keep the default JSON beside the
adapter source and embed it in the binary so existing deployments start
without a new mount. An explicit `PLATFORM_CONFIG_PATH` overrides it.

Use bounded synchronous file reading during startup, before the application
serves requests. Deserialize with Serde and validate the complete profile.
Return only the typed profile, never arbitrary configuration properties.

The endpoint uses the adapter's existing public GET and CORS boundary.
No new service, database, signing job, event or provider command is needed.
External-profile fields are presentation hints; existing profile persistence
does not become a generic external-identity system.

Place public DTOs in generated-contracts and describe them in OpenAPI.
Brief types use a bounded string on storage/read and an allowed-value check
on HTTP writes against configured labels plus the six legacy values.
Currency metadata must not silently introduce a scale into current amounts.
