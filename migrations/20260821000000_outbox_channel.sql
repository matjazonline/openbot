-- The channel an outbound email goes out as, promoted from the JSONB payload to a real column.
--
-- The payload is the poller's data, not a queryable dimension: filtering the outbox by channel
-- meant an unindexed JSONB scan, which is why the Outbox workspace offered one fewer filter than
-- the Tasks workspace beside it. Every neighbouring table (`background_tasks`, `human_approvals`)
-- already carries `channel_id` this way.
ALTER TABLE email_outbox ADD COLUMN channel_id UUID;

-- Backfill by joining against `channels` rather than casting the JSON text: a row whose payload
-- names a channel that no longer exists — or never held a uuid at all — is left NULL instead of
-- failing the cast here or the constraint below.
UPDATE email_outbox outbox
SET channel_id = channel.id
FROM channels channel
WHERE channel.company_id = outbox.company_id
  AND channel.id::text = outbox.payload->>'channel_id';

-- Compound, like `email_outbox_task_fk`: the channel must belong to the same company as the email.
-- Deleting a channel must not delete the record that mail was queued for it, hence SET NULL.
ALTER TABLE email_outbox
    ADD CONSTRAINT email_outbox_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE SET NULL (channel_id);

CREATE INDEX email_outbox_company_channel_created_idx
    ON email_outbox (company_id, channel_id, created_at DESC, id DESC);
