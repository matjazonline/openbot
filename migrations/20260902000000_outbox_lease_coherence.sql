-- Repair rows written before lease coherence was enforced. A malformed sending row consumed a
-- delivery attempt because no worker can legitimately finish it; other statuses merely shed stale
-- lease metadata.
UPDATE email_outbox
SET status = CASE WHEN retry_count + 1 >= 5 THEN 'failed' ELSE 'pending' END,
    retry_count = retry_count + 1,
    last_error = 'Invalid delivery lease repaired during migration',
    available_at = CURRENT_TIMESTAMP,
    worker_id = NULL,
    locked_at = NULL,
    lock_expires_at = NULL,
    updated_at = CURRENT_TIMESTAMP
WHERE status = 'sending'
  AND (worker_id IS NULL OR locked_at IS NULL OR lock_expires_at IS NULL
       OR lock_expires_at <= locked_at);

UPDATE email_outbox
SET worker_id = NULL,
    locked_at = NULL,
    lock_expires_at = NULL,
    updated_at = CURRENT_TIMESTAMP
WHERE status <> 'sending'
  AND (worker_id IS NOT NULL OR locked_at IS NOT NULL OR lock_expires_at IS NOT NULL);

ALTER TABLE email_outbox
    ADD CONSTRAINT email_outbox_lease_check CHECK (
        (status = 'sending'
         AND worker_id IS NOT NULL
         AND locked_at IS NOT NULL
         AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'sending'
         AND worker_id IS NULL
         AND locked_at IS NULL
         AND lock_expires_at IS NULL)
    ) NOT VALID;

ALTER TABLE email_outbox VALIDATE CONSTRAINT email_outbox_lease_check;
