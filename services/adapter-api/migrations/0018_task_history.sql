-- Dormant history support for the later API task writer. Existing enqueues remain
-- provider-owned; this release never executes API task work.
ALTER TABLE operations ADD COLUMN IF NOT EXISTS task_operation BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE operations ADD COLUMN IF NOT EXISTS task_result_json TEXT;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='operations'::regclass AND conname='operations_task_source') THEN
        ALTER TABLE operations DROP CONSTRAINT IF EXISTS operations_status_check;
        ALTER TABLE operations ADD CONSTRAINT operations_task_source CHECK (
            (NOT task_operation
                AND status IN ('AwaitingSignature','ReadyToSubmit','Submitted','OutcomeUnknown','Finalized','Rejected','Expired')
                AND task_result_json IS NULL)
            OR (task_operation
                AND status IN ('Queued','Finalized','Rejected','Expired')
                AND provider_instance_id IS NOT NULL
                AND signable_payload IS NULL AND signed_json IS NULL AND receipt_json IS NULL
                AND NOT possibly_submitted AND NOT catalog_operation
                AND ((status='Finalized') = (task_result_json IS NOT NULL)))
        );
    END IF;
END $$;

-- API task notifications share the existing notification ID sequence. A NULL
-- provider cursor means no provider event is asserted, never a made-up cursor.
ALTER TABLE notifications ALTER COLUMN provider_cursor DROP NOT NULL;
ALTER TABLE notifications ADD COLUMN IF NOT EXISTS api_operation_id TEXT REFERENCES operations(operation_id);
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='notifications'::regclass AND conname='notifications_event_source') THEN
        ALTER TABLE notifications ADD CONSTRAINT notifications_event_source CHECK (
            (provider_cursor IS NOT NULL AND api_operation_id IS NULL)
            OR (provider_cursor IS NULL AND api_operation_id IS NOT NULL)
        );
    END IF;
END $$;
CREATE UNIQUE INDEX IF NOT EXISTS notifications_api_operation
    ON notifications(account_id,provider_instance_id,api_operation_id) WHERE api_operation_id IS NOT NULL;
