# Secrets Policy

Custody stores wallet seeds encrypted with XChaCha20-Poly1305 in its own SQLite database. A separate runtime master-key file protects them; the system-origin seed is another private file. Service tokens and bootstrap credentials are also private runtime inputs. Browsers receive none of these values.

Use the generator documented in [infra/README.md](../../infra/README.md). It creates private files without printing their contents and refuses existing targets. Keep them outside the repository/build context. Compose mounts only each service's required files.

The backend signs with sr25519 in custody memory. HSM-backed signing, DEK/KEK envelope encryption and automatic or manual encryption-key rotation are not implemented. Their discussion is not an approved replacement for SPEC-0001's unresolved recovery requirement.

Do not replace a master-key file on an existing database: current ciphertext would become unreadable. Changing a login password is separate from wallet or encryption-key changes. Wallet suspension/retirement is documented in [custody](../../services/wallet/README.md).

Never put secrets in logs, issues, commits, browser configuration or chat. Full process/host compromise can expose or use POC keys. No real assets are allowed; see [incident response](incident-response.md).
