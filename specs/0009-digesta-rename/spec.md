# SPEC-0009: Digesta project name

Status: APPROVED

Owner: Digesta product owner

Task: RENAME-001

Approval: The product owner requested the project rename to Digesta, including published package names, on 2026-10-15.

## Requirements

- NAME-001: Use Digesta in current product prose, frontend branding and API metadata.
- NAME-002: Use `digesta-*` for local and published container images. GHCR publishing follows the repository owner and retains the existing tag trigger and verification gates.
- NAME-003: Rename branded development defaults, temporary test paths, browser configuration and the session cookie consistently across implementation, contracts, tests and documentation.
- NAME-004: Preserve cryptographic format bytes and historical evidence identifying actual paths, graph indexes, deployed images and prior cookies. Explain compatibility-critical identifiers in the environment guide.
- NAME-005: Document changes needed by existing local environments without moving secrets, renaming databases, deleting data, changing GitHub ownership or publishing a release.

## Compatibility

The session cookie becomes `digesta_session`; existing browsers must log in again. Its security attributes, CSRF checks and authorization rules remain the same. The browser configuration becomes `window.DIGESTA_API_BASE`, and the container configuration mount becomes `/etc/digesta/config.js`.

New local environments use `DIGESTA_LOCAL_DIR`, the `digesta-local` Compose project and Digesta PostgreSQL defaults. Existing environments must retain their actual database connection and Compose project when keeping stored data. Cryptographic domains, associated data and the KVN token unit remain unchanged.

## Verification

Inventory case-insensitive occurrences and review every exception. Validate OpenAPI, Compose and Nginx configuration; run the applicable Rust gates and WASM check. Record unavailable infrastructure and tools precisely in the PR.
