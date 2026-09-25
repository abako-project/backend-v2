-- Adapter-only authentication state. Ceremony JSON never leaves PostgreSQL.
ALTER TABLE principals ADD COLUMN IF NOT EXISTS passkey_user_id TEXT UNIQUE;

CREATE TABLE IF NOT EXISTS passkey_credentials (
    credential_ref TEXT PRIMARY KEY,
    credential_id BYTEA NOT NULL UNIQUE,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    passkey_json TEXT NOT NULL,
    sign_count BIGINT NOT NULL DEFAULT 0 CHECK (sign_count >= 0),
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS passkeys_principal ON passkey_credentials(principal_id);

CREATE TABLE IF NOT EXISTS passkey_ceremonies (
    ceremony_id TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(principal_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('register', 'login')),
    state_json TEXT NOT NULL,
    session_token_hash BYTEA CHECK (session_token_hash IS NULL OR octet_length(session_token_hash) = 32),
    expires_at BIGINT NOT NULL,
    CHECK ((kind = 'register') = (session_token_hash IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS passkey_ceremonies_expiry ON passkey_ceremonies(expires_at);
