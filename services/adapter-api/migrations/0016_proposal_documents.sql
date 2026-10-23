-- Exact immutable proposal-context preimages. JSON text preserves NUL/Unicode.
-- Numeric facts, assigned revisions and lifecycle remain in the signed core.
CREATE TABLE IF NOT EXISTS proposal_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY (provider_instance_id, content_hash)
);
