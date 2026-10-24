-- Explicit immutable context preimages. JSON text preserves accepted Unicode/NUL.
CREATE TABLE IF NOT EXISTS catalog_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY(provider_instance_id,content_hash)
);
CREATE TABLE IF NOT EXISTS lifecycle_reason_documents (
    provider_instance_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL CHECK (octet_length(content_hash) = 32),
    payload TEXT NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 33554432),
    PRIMARY KEY(provider_instance_id,content_hash)
);
-- One narrow lane in the existing operation queue, not a second queue/worker.
ALTER TABLE operations ADD COLUMN IF NOT EXISTS catalog_operation BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX IF NOT EXISTS operations_catalog_sequence ON operations(creation_sequence) WHERE catalog_operation;
-- The digest bounds the B-tree key; compare the complete normalized UTF-8 bytes.
-- A rename reserves its new name while the old committed row remains selected.
CREATE TABLE IF NOT EXISTS catalog_skill_names (
    provider_instance_id TEXT NOT NULL,
    name_hash BYTEA NOT NULL CHECK (octet_length(name_hash) = 32),
    normalized BYTEA NOT NULL CHECK (octet_length(normalized) BETWEEN 1 AND 33554432),
    skill_id BIGINT CHECK(skill_id BETWEEN 1 AND 4294967295),
    label_hash BYTEA CHECK(octet_length(label_hash) = 32),
    origin_hash BYTEA CHECK(octet_length(origin_hash) = 32),
    reservation_op TEXT REFERENCES operations(operation_id),
    PRIMARY KEY(provider_instance_id,name_hash),
    CHECK ((skill_id IS NULL) = (label_hash IS NULL)),
    CHECK ((skill_id IS NULL) = (origin_hash IS NULL)),
    CHECK (skill_id IS NOT NULL OR reservation_op IS NOT NULL)
);
CREATE UNIQUE INDEX IF NOT EXISTS catalog_skill_name_identity ON catalog_skill_names(provider_instance_id,skill_id) WHERE skill_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS catalog_skill_name_reservation ON catalog_skill_names(provider_instance_id,reservation_op) WHERE reservation_op IS NOT NULL;
