-- The original request already retains task text/progress. A later writer may
-- classify it as a core membership operation in this same queue row. Keeping
-- project context across that transition fences edits until API materialization.
-- This schema preparation does not enable classification or change public writes.
ALTER TABLE operations DROP CONSTRAINT IF EXISTS operations_task_project_source;
ALTER TABLE operations ADD CONSTRAINT operations_task_project_source CHECK (
    task_project_id IS NULL OR (
        provider_instance_id IS NOT NULL
        AND request_command_bytes IS NOT NULL
        AND NOT catalog_operation
    )
);
CREATE INDEX IF NOT EXISTS operations_task_project_order
    ON operations(provider_instance_id,task_project_id,creation_sequence)
    WHERE task_project_id IS NOT NULL;
