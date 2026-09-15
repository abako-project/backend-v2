# Disaster Recovery

There is no verified production disaster-recovery procedure, RPO or RTO for this POC. Do not use its keys for real assets.

A recoverable local environment includes all three service databases and their matching private runtime secrets. Custody ciphertext alone is insufficient: losing its master key loses access to the stored wallet seeds. Preserve the configured system-origin seed and identity too.

For a local offline backup, stop the full stack before copying the coherent secret/data directory to protected storage. Do not treat a copy of live SQLite files as a verified backup. Restart using the same directory and environment configuration.

Restoring only mock state or only adapter state can invalidate operation history and provider identity relationships. A fresh demo should use a fresh complete environment instead of mixing generations.

Automated encrypted backups, retention, restore drills, key-version recovery and regional failover remain undefined. Master-key rotation remains an open SPEC-0001 reconciliation item; no rotation command is available.
