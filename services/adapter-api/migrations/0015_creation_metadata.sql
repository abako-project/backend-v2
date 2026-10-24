-- R2a preparation: immutable request preimages captured before signing.
-- Public creation still uses its predecessor command until the paired core
-- cutover. JSON text preserves accepted control characters, including NUL.
CREATE TABLE IF NOT EXISTS worker_directory_metadata (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);
CREATE TABLE IF NOT EXISTS project_creation_metadata (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);
