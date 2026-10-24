-- Expand only: old binaries may continue inserting operations without this field.
ALTER TABLE operations ADD COLUMN IF NOT EXISTS request_command_bytes BYTEA;

-- Text records retain their existing adapter tables and identity scopes.
-- URL records submitted with signed operations live here instead of onchain.
CREATE TABLE IF NOT EXISTS evidence_url_records (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    purpose SMALLINT NOT NULL CHECK (purpose BETWEEN 0 AND 3),
    target_id TEXT NOT NULL,
    record_id TEXT NOT NULL,
    author_account TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 8192),
    PRIMARY KEY (provider_instance_id, project_id, purpose, target_id, record_id)
);
