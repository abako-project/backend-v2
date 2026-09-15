CREATE TABLE IF NOT EXISTS custodial_wallets (
    wallet_id TEXT PRIMARY KEY NOT NULL,
    principal_id TEXT UNIQUE,
    account_id TEXT UNIQUE NOT NULL,
    scheme TEXT NOT NULL DEFAULT 'sr25519' CHECK(scheme = 'sr25519'),
    encrypted_seed BLOB NOT NULL CHECK(length(encrypted_seed) = 48),
    encryption_nonce BLOB NOT NULL CHECK(length(encryption_nonce) = 24),
    encryption_format_version INTEGER NOT NULL DEFAULT 1 CHECK(encryption_format_version = 1),
    master_key_version INTEGER NOT NULL DEFAULT 1 CHECK(master_key_version = 1),
    lifecycle TEXT NOT NULL CHECK(lifecycle IN ('Provisioning', 'Active', 'Suspended', 'Retired')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(master_key_version, encryption_nonce)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_system_wallet ON custodial_wallets((1)) WHERE principal_id IS NULL;
CREATE TABLE IF NOT EXISTS signing_jobs (
    creation_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT UNIQUE NOT NULL,
    wallet_id TEXT NOT NULL REFERENCES custodial_wallets(wallet_id),
    payload_version INTEGER NOT NULL CHECK(payload_version = 1),
    signable_payload BLOB NOT NULL CHECK(length(signable_payload) BETWEEN 1 AND 262144),
    payload_hash BLOB NOT NULL CHECK(length(payload_hash) = 32),
    expires_at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('Pending', 'Signed', 'Rejected')),
    signature BLOB CHECK(length(signature) = 64),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 5),
    next_attempt_at INTEGER NOT NULL,
    lease_token TEXT UNIQUE,
    lease_until INTEGER,
    rejection_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'Signed') = (signature IS NOT NULL)),
    CHECK((status = 'Rejected') = (rejection_code IS NOT NULL)),
    CHECK(status = 'Pending' OR lease_token IS NULL)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_claim_per_wallet ON signing_jobs(wallet_id) WHERE lease_token IS NOT NULL;
CREATE INDEX IF NOT EXISTS pending_signing_jobs ON signing_jobs(status, next_attempt_at, creation_sequence);
CREATE TABLE IF NOT EXISTS custody_audit_records (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT,
    wallet_id TEXT,
    event_kind TEXT NOT NULL,
    result_code TEXT NOT NULL,
    payload_hash BLOB,
    occurred_at INTEGER NOT NULL,
    details TEXT NOT NULL DEFAULT '{}' CHECK(length(details) <= 1024)
);
CREATE INDEX IF NOT EXISTS audit_operation ON custody_audit_records(operation_id, occurred_at);
