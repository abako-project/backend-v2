# Digesta rename

The product is Digesta. Current branding, API metadata, frontend configuration,
local defaults, tests and release package names use that name. Task RENAME-001
implements [SPEC-0009](../../specs/0009-digesta-rename/spec.md).

## Published images

The next successful version-tag release publishes `digesta-adapter-api`,
`digesta-wallet` and `digesta-mock-provider` under
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

## Inventory and retained occurrences

The original tracked inventory contained 152 occurrences across 51 files. The
repository was searched case-insensitively, including hidden tracked files.
The working checkout was also searched for ignored text outside build outputs
and secret directories. No tracked filenames contain the previous names.

| Location | Decision |
|---|---|
| Frontend titles, descriptions and header; README and current product prose | Rename to Digesta. |
| OpenAPI and adapter session cookie; contract test and auth test helper | Use `digesta_session` consistently. |
| Browser runtime config, Nginx alias and frontend image copy path | Use `DIGESTA_API_BASE` and `/etc/digesta/config.js`. |
| Compose project, images, development database defaults and secret/data variable | Use Digesta defaults for new environments. |
| Release workflow, release guide and Quadlet examples | Use `digesta-*` packages under the runtime repository owner. |
| Cargo repository placeholder and temporary test paths | Rename to Digesta; no Cargo package identifiers contained either previous name. |
| `.gitignore` | Add `/digesta-web/`; retain `/abako-web/` to keep an existing independent checkout ignored. |
| `crates/generated-contracts/src/lib.rs` | Preserve `KUNVENO-MOCK-V1!`: changing signed protocol bytes invalidates existing calls and receipts. |
| `services/wallet/src/crypto.rs` | Preserve `kunveno:custody:sr25519:format=1:key=1:`: changing authenticated data prevents existing wallet seeds from decrypting. |
| `progress/handoffs/` | Preserve literal worktree/log paths, graph names, deployed images and configuration reported by past verification. |
| SPEC-0001 design and proposal-review deployment evidence | Preserve the actual indexed graph and Compose project names. |
| Ignored `notes/bloque-ramp-research.md` in the original checkout | Local research references an independent `../abako-web` checkout. It is outside the tracked repository and is not shipped. |
| Git remote, checkout directory and external resources | Remain at their actual locations; this task does not transfer GitHub ownership or move external checkouts. |

This inventory intentionally names the old identifiers so future searches can
distinguish compatibility requirements and historical evidence from missed
product branding.
