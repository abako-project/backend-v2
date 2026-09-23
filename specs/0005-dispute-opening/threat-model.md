# Dispute opening: threat model

Status: IMPLEMENTED — 2026-09-23

| Threat | Control and evidence |
|---|---|
| Spoofed author / foreign submission | Derive signed origin; validate ancestry/current review in provider; test foreign IDs and non-parties |
| Stale review after resubmission | Bind accept/reject to current submission ID; test stale commands |
| Mutation through overlooked route | Central project freeze; test all command families, other milestones and root origin |
| Concurrent opening/response/replay | Atomic provider commit and uniqueness; exact replay returns original receipt; different IDs cannot overwrite |
| Public route exposes writes | Match public GET/HEAD only; test unauthenticated writes and invalid CSRF |
| Internal data leak | Allowlisted case DTO; no credentials, signed bytes, notification metadata or unrelated projects |
| SSRF through evidence | No backend URL fetch/preview/webhook; structural validation only |
| Changed/missing artifact | SHA-256 detects different bytes when retrievable; no retention or automatic integrity-verification claim |
| Credentials in reference/log | Reject URL userinfo/control characters; never log URLs; no promise to detect arbitrary secrets in text |
| Malformed persisted/encoded state | JSON/SCALE validation and restore invariants; incompatible state fails explicitly |
| Unilateral payout/unlock | No resolution API or root bypass; negative tests preserve funds and reservations |

No production retention/erasure service, DAO adjudication or encrypted chat is
implied. Onchain references/accounts remain public regardless of adapter login.
A digest does not certify authorship, availability or fulfilment of requirements;
the signed operation records who supplied the commitment.
