# Incident Response

This POC has no production incident organisation or automated response system. The local operator owns containment and evidence preservation.

If a custody seed, master key or service credential may be compromised, stop the affected local environment, restrict access to its secret/data directory and preserve relevant state and redacted diagnostics. Do not distribute raw logs or databases as incident reports.

Record the affected environment, revision, operation IDs, suspected exposure and containment actions. Investigate the source before restarting or creating a fresh coherent demo environment.

Master-key rotation is not implemented. Replacing a file does not re-encrypt stored seeds, and re-encrypting seeds would not recover wallets whose signing secrets were already stolen. Wallet-compromise migration is a separate future procedure.

Recovery and root-cause review must cover credentials, databases, build inputs and host access. Production severity levels, on-call ownership, notification duties and recovery exercises require a separate deployment decision.
