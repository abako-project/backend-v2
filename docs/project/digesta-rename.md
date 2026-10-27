# Digesta rename

The product is Digesta. Current branding, API metadata, frontend configuration,
local defaults, tests and release package names use that name. Task RENAME-001
implements [SPEC-0009](../../specs/0009-digesta-rename/spec.md).

## Published images

The next successful version-tag release publishes `digesta-api`,
`digesta-wallet` and `digesta-native-provider` under
`ghcr.io/<repository-owner>/`. The workflow lowercases the actual GitHub owner;
it does not create or rename an organization. The current Git remote belongs
to `abako-project`, so the current destination is
`ghcr.io/abako-project/digesta-*` until the repository changes ownership.

Update deployment image references to the new packages after they are
published. Existing registry packages are not renamed or deleted by this
change. Tag filters, verification gates and semver tags remain the same.

## Existing local environments

Fresh environments use `DIGESTA_LOCAL_DIR`, Compose project `digesta-local`,
PostgreSQL user `digesta` and database `digesta_adapter`. For an existing
environment:

1. Set `DIGESTA_LOCAL_DIR` to the actual existing secrets/data directory. The
   directory may keep its old name. Do not regenerate secrets or move live data.
2. Keep the existing Compose project with `COMPOSE_PROJECT_NAME` or `-p` so
   Compose retains the same PostgreSQL volume. A changed project name selects
   a different volume.
3. Set `ADAPTER_DATABASE_URL` to the existing database connection. New
   PostgreSQL initialization defaults do not rename users or databases in an
   existing volume. If retaining old database names, use a local Compose
   override for `POSTGRES_USER`, `POSTGRES_DB`, `POSTGRES_PASSWORD` and the
   matching `pg_isready` health check. Retain the original credentials privately.
4. Update custom frontend configuration to `window.DIGESTA_API_BASE`. Custom
   container mounts for `/config.js` now target `/etc/digesta/config.js`.
5. Rebuild the affected images and log in again. The session cookie is now
   `digesta_session`; previous cookies are not consumed. Cookie security
   attributes, CSRF enforcement and authorization rules are unchanged.

KVN remains the simulated token unit. No money or pricing rule changes.

## Stable identifiers

The signed prefix `KUNVENO-MOCK-V1!` and custody authentication context
`kunveno:custody:sr25519:format=1:key=1:` intentionally retain their bytes.
Changing display/component names must not invalidate accepted signatures or
prevent wallet secrets from decrypting. Historical rename inventory remains in Git.

Current content/evidence commitment hash prefixes also retain their original
bytes. The owner confirmed on 2026-10-27 that this branding cleanup preserves
working repository/registry URLs, deployed hostnames and protocol identifiers.
Those technical references are not product display names.
Pinned contract bundles retain their recorded contributor metadata and complete
verified bytes: editing those labels changes the bundle identity used by saved
checkpoints. Current contract manifests use Digesta for future builds.
