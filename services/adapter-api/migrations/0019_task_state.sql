-- Final-purpose task state, prepared from one provider export. No task ownership
-- switches in this migration, and public reads still use the selected core copy.
CREATE TABLE IF NOT EXISTS task_generations (
    provider_instance_id TEXT PRIMARY KEY,
    next_task_id BIGINT NOT NULL CHECK(next_task_id BETWEEN 1 AND 4294967295),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    storage_count BIGINT NOT NULL CHECK(storage_count>=0),
    task_count BIGINT NOT NULL CHECK(task_count>=0)
);
CREATE TABLE IF NOT EXISTS task_storages (
    provider_instance_id TEXT NOT NULL REFERENCES task_generations(provider_instance_id) ON DELETE CASCADE,
    task_storage_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    proposal_id TEXT NOT NULL,
    proposal_revision TEXT NOT NULL,
    milestone_id TEXT NOT NULL,
    PRIMARY KEY(provider_instance_id,task_storage_id),
    UNIQUE(provider_instance_id,project_id,proposal_id,milestone_id)
);
CREATE INDEX IF NOT EXISTS task_storages_project ON task_storages(provider_instance_id,project_id);
CREATE TABLE IF NOT EXISTS task_items (
    provider_instance_id TEXT NOT NULL,
    task_id BIGINT NOT NULL CHECK(task_id BETWEEN 1 AND 4294967295),
    task_storage_id TEXT NOT NULL,
    ordinal BIGINT NOT NULL CHECK(ordinal>=0),
    payload TEXT NOT NULL,
    assignees TEXT[] NOT NULL,
    PRIMARY KEY(provider_instance_id,task_id),
    UNIQUE(provider_instance_id,task_storage_id,ordinal),
    FOREIGN KEY(provider_instance_id,task_storage_id) REFERENCES task_storages(provider_instance_id,task_storage_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS operations_task_generation ON operations(provider_instance_id) WHERE task_operation;
