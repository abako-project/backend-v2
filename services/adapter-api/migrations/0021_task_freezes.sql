-- Typed Rust fills this projection for relevant queued API work; JSON processing
-- in PostgreSQL would reject retained NUL-bearing task narrative.
ALTER TABLE operations ADD COLUMN IF NOT EXISTS task_project_id TEXT;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='operations_task_project_source' AND conrelid='operations'::regclass) THEN
        ALTER TABLE operations ADD CONSTRAINT operations_task_project_source CHECK(task_project_id IS NULL OR task_operation);
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS task_freezes (
    operation_id TEXT PRIMARY KEY REFERENCES operations(operation_id),
    provider_instance_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('Cancellation','Dispute')),
    state TEXT NOT NULL DEFAULT 'Pending' CHECK(state IN ('Pending','Confirmed','Released')),
    snapshot_json TEXT NOT NULL,
    FOREIGN KEY(provider_instance_id,project_id) REFERENCES task_projects(provider_instance_id,project_id)
);
CREATE INDEX IF NOT EXISTS task_freezes_project ON task_freezes(provider_instance_id,project_id) WHERE state<>'Released';
