# Supply-Chain Security

The workspace commits Cargo.lock and uses locked builds. Dependency selection records are in [poc-baseline.md](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies/poc-baseline.md); detailed package findings are in [the dependency audit](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/docs/dependencies/poc-audit-2026-09-10.md).

```sh
cargo deny check
cargo audit
```

## Current dependency findings

On 2026-10-21, `cargo deny check` against the unchanged lockfile failed advisories
and license policy; bans and sources passed. The current result supersedes older
reports limited to license exclusions and two maintenance warnings.

| Locked package | Vulnerability advisories reported |
|---|---|
| `rustls 0.23.45` | [RUSTSEC-2026-0346](https://rustsec.org/advisories/RUSTSEC-2026-0346.html), [0347](https://rustsec.org/advisories/RUSTSEC-2026-0347.html), [0348](https://rustsec.org/advisories/RUSTSEC-2026-0348.html), [0349](https://rustsec.org/advisories/RUSTSEC-2026-0349.html) |
| `rustls-webpki 0.103.15` | [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html) |
| `webpki-root-certs 1.0.6` | [RUSTSEC-2026-0383](https://rustsec.org/advisories/RUSTSEC-2026-0383.html) |

These are dependency-graph findings, not proof that every affected feature or
exploit path is active in this application. Applicability and upgrades need a
separate security change. The cleanup does not alter dependencies or suppress
findings. The earlier deferral of named maintenance warnings does not accept
these vulnerability advisories.

The check also reports unmaintained `paste` and `proc-macro-error2` and rejected
CC0-1.0/Boost BSL-1.0 license expressions under the checked-in policy. `cargo audit`
was unavailable in this environment, so no fresh passing audit result is claimed.
Application checks and dependency-policy checks are separate; see
[verification](../../ci/README.md).

Docker frontend builds use wasm-bindgen-cli directly; the earlier Trunk build-tool graph was removed. Application-lock auditing does not by itself audit separately installed build tools or container packages.

Unsafe/native/cryptographic dependencies require the review defined in AGENTS.md. Production image scans, published SBOMs, signed provenance and artifact signing are not implemented release guarantees.

## Project metadata decision

The root package metadata declares `Apache-2.0`, while
[LICENSE-TEMPLATE-NOTICE.md](../../LICENSE-TEMPLATE-NOTICE.md) describes a placeholder
`Apache-2.0 OR MIT` choice and asks for the actual license texts. The cleanup
preserves both pending an explicit owner decision. It does not select a license
or resolve dependency license-policy findings.
