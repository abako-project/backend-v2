CREATE TABLE IF NOT EXISTS principals (
    principal_id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    is_admin INTEGER NOT NULL CHECK (is_admin IN (0, 1)),
    wallet_id TEXT UNIQUE,
    account_id TEXT UNIQUE,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    principal_id TEXT NOT NULL REFERENCES principals(principal_id),
    csrf_token TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_principal ON sessions(principal_id);
CREATE TABLE IF NOT EXISTS operations (
    creation_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id),
    wallet_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    command_json TEXT NOT NULL,
    command_bytes BLOB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('AwaitingSignature', 'ReadyToSubmit', 'Submitted', 'OutcomeUnknown', 'Finalized', 'Rejected', 'Expired')),
    signable_payload BLOB,
    signed_json TEXT,
    provider_instance_id TEXT,
    receipt_json TEXT,
    error_code TEXT,
    possibly_submitted INTEGER NOT NULL DEFAULT 0 CHECK (possibly_submitted IN (0, 1)),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL,
    CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);
CREATE UNIQUE INDEX IF NOT EXISTS operations_wallet_lease ON operations(wallet_id) WHERE lease_token IS NOT NULL;
CREATE INDEX IF NOT EXISTS operations_queue ON operations(status, next_attempt_at, creation_sequence);
CREATE TABLE IF NOT EXISTS event_cursors (
    provider_instance_id TEXT PRIMARY KEY,
    cursor INTEGER NOT NULL CHECK (cursor >= 0)
);
CREATE TABLE IF NOT EXISTS notifications (
    notification_id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    provider_cursor INTEGER NOT NULL,
    event_json TEXT NOT NULL,
    read_at INTEGER,
    UNIQUE (account_id, provider_instance_id, provider_cursor)
);
CREATE INDEX IF NOT EXISTS notifications_account ON notifications(account_id, notification_id);
CREATE TABLE IF NOT EXISTS audit_records (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal_id TEXT,
    operation_id TEXT,
    event_kind TEXT NOT NULL,
    result_code TEXT NOT NULL,
    occurred_at INTEGER NOT NULL
);
