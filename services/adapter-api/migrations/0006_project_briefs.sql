CREATE TABLE IF NOT EXISTS project_briefs (
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    brief JSONB NOT NULL,
    PRIMARY KEY (provider_instance_id, project_id)
);
