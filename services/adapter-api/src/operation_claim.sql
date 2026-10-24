-- A pending project hold suspends API task execution and its FIFO blocking role.
-- All remaining work keeps its order, including intervening provider operations
-- and cross-wallet freeze cycles. A confirmed freeze takes precedence: tasks
-- must then run to record their known failure, even if another hold is pending.
WITH suspended_tasks AS (
    SELECT task.operation_id
    FROM operations task
    JOIN task_generations generation ON generation.provider_instance_id=task.provider_instance_id AND generation.api_owned
    WHERE task.task_operation AND task.status='Queued'
      AND EXISTS (
          SELECT 1 FROM task_freezes hold
          WHERE hold.provider_instance_id=task.provider_instance_id
            AND hold.project_id=task.task_project_id AND hold.state='Pending'
      )
      AND NOT EXISTS (
          SELECT 1 FROM task_freezes hold
          WHERE hold.provider_instance_id=task.provider_instance_id
            AND hold.project_id=task.task_project_id AND hold.state='Confirmed'
      )
)
SELECT * FROM operations o
WHERE (NOT o.task_operation OR ($2 AND EXISTS (
    SELECT 1 FROM task_generations g WHERE g.provider_instance_id=o.provider_instance_id AND g.api_owned
)))
AND o.status NOT IN ('Finalized','Rejected','Expired')
AND (o.lease_token IS NULL OR o.lease_until<=$1)
AND o.next_attempt_at<=$1
AND NOT EXISTS (SELECT 1 FROM suspended_tasks s WHERE s.operation_id=o.operation_id)
AND NOT EXISTS (
    SELECT 1 FROM operations previous
    WHERE (previous.wallet_id=o.wallet_id OR (o.catalog_operation AND previous.catalog_operation))
      AND previous.creation_sequence<o.creation_sequence
      AND (
          previous.status NOT IN ('Finalized','Rejected','Expired')
          OR (o.catalog_operation AND previous.catalog_operation AND previous.status='Expired' AND previous.possibly_submitted)
          OR EXISTS (SELECT 1 FROM task_freezes hold WHERE hold.operation_id=previous.operation_id AND hold.state='Pending')
      )
      AND NOT EXISTS (SELECT 1 FROM suspended_tasks s WHERE s.operation_id=previous.operation_id)
)
ORDER BY o.creation_sequence LIMIT 1 FOR UPDATE OF o SKIP LOCKED
