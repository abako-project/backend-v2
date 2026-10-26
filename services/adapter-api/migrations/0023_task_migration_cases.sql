-- Immutable predecessor case context. These rows are populated only by the
-- paused owner migration, never by public reads or post-cutover event ingestion.
CREATE TABLE IF NOT EXISTS task_migration_cases (
    provider_instance_id TEXT NOT NULL REFERENCES task_generations(provider_instance_id),
    case_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    payload_hash BYTEA NOT NULL CHECK(octet_length(payload_hash)=32),
    PRIMARY KEY(provider_instance_id,case_id)
);
