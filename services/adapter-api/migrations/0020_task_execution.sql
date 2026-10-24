-- The prepared copy remains provider-owned. Only a later verified cutover may
-- establish API ownership; this milestone exposes no activation command.
ALTER TABLE task_generations ADD COLUMN IF NOT EXISTS api_owned BOOLEAN NOT NULL DEFAULT FALSE;

-- Task commits and the later dispute snapshot/hold admission use this same row.
CREATE TABLE IF NOT EXISTS task_projects (
    provider_instance_id TEXT NOT NULL REFERENCES task_generations(provider_instance_id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,
    PRIMARY KEY(provider_instance_id,project_id)
);
