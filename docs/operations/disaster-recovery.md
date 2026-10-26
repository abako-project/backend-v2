# Disaster Recovery

There is no verified production disaster-recovery procedure, RPO or RTO for this POC. Do not use its keys for real assets.

A recoverable local environment includes all three service databases and their matching private runtime secrets. Custody ciphertext alone is insufficient: losing its master key loses access to the stored wallet seeds. Preserve the configured system-origin seed and identity too.

A backup must include adapter PostgreSQL as well as custody/mock SQLite and
matching secrets. Copying `DIGESTA_LOCAL_DIR` alone omits the Compose PostgreSQL
named volume. Follow the coherent backup/restore procedure in the
[preview operations guide](digesta-backend-preview.md) for that deployment.

For local preservation, stop application writes, preserve PostgreSQL with its
dump/restore or stopped-volume procedure, and copy the stopped custody/mock state
and secrets together to protected storage. Do not treat copied live SQLite files
or only the bind-mounted directory as a complete backup. Restore the matching
stores and original configuration before restarting.

Restoring only mock state or only adapter state can invalidate operation history and provider identity relationships. A fresh demo should use a fresh complete environment instead of mixing generations.

The preview guide records a verified isolated restore for that mock deployment.
Automated encrypted backups, retention, production restore drills, key-version
recovery and regional failover remain undefined. Master-key rotation remains an open SPEC-0001 reconciliation item; no rotation command is available.

Directory names, project text, proposal documents and task content live in
adapter PostgreSQL. The core archive cannot reconstruct those preimages.
Preserve the complete API backup with the current core archive and wallet state.
The owner-only metadata exchange exports selected confirmed content, not sessions,
pending operations or the full API backup. Follow the [current paired recovery
and rollback procedure](task-cutover.md) with paused/reconciled operations, fresh
destinations and the matching predecessor binary. Earlier format transitions are
[historical instructions](https://github.com/abako-project/backend-v2/blob/1964a56d88bc31f652af7151841023c6aa53e896/specs/0015-adapter-core-boundary/r2b-proposals.md),
not the current recovery procedure. This does not establish a production
disaster-recovery guarantee.
