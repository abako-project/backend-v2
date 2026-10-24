-- R1 copies only. The provider remains authoritative until explicit cutover.
-- Fingerprints select the exact source version, avoiding stale snapshot overwrites.
CREATE TABLE IF NOT EXISTS project_content_copies (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    source_fingerprint BYTEA NOT NULL CHECK (octet_length(source_fingerprint) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, project_id, source_fingerprint)
);
CREATE TABLE IF NOT EXISTS worker_directory_copies (
    provider_instance_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    source_fingerprint BYTEA NOT NULL CHECK (octet_length(source_fingerprint) = 32),
    display_name_json TEXT NOT NULL CHECK (octet_length(display_name_json) BETWEEN 2 AND 33554432),
    PRIMARY KEY (provider_instance_id, account_id, source_fingerprint)
);
