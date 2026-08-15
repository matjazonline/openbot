-- The outreach tables were previously unused scaffolding. This feature intentionally
-- starts with a clean normalized model and does not migrate draft JSON-era state.
DELETE FROM task_outreaches;

ALTER TABLE task_outreaches
    DROP CONSTRAINT task_outreaches_kind_check,
    DROP CONSTRAINT task_outreaches_status_check,
    DROP CONSTRAINT task_outreaches_threshold_check,
    DROP COLUMN kind,
    ADD COLUMN outreach_key TEXT,
    ADD COLUMN subject TEXT,
    ADD COLUMN body TEXT;

UPDATE task_outreaches
SET outreach_key = id::text,
    subject = '',
    body = '';

ALTER TABLE task_outreaches
    ALTER COLUMN outreach_key SET NOT NULL,
    ALTER COLUMN subject SET NOT NULL,
    ALTER COLUMN body SET NOT NULL,
    ALTER COLUMN required_threshold_percent SET NOT NULL,
    ALTER COLUMN expires_at SET NOT NULL,
    ADD CONSTRAINT task_outreaches_task_key UNIQUE (task_id, outreach_key),
    ADD CONSTRAINT task_outreaches_status_check CHECK (
        status IN (
            'waiting', 'threshold_met', 'timeout_pending_approval',
            'proceed_partial', 'cancelled', 'completed'
        )
    ),
    ADD CONSTRAINT task_outreaches_threshold_check CHECK (
        required_threshold_percent > 0
        AND required_threshold_percent <= 100
    ),
    ADD CONSTRAINT task_outreaches_expiry_check CHECK (expires_at > created_at),
    ADD CONSTRAINT task_outreaches_subject_check CHECK (length(btrim(subject)) > 0),
    ADD CONSTRAINT task_outreaches_body_check CHECK (length(btrim(body)) > 0);

ALTER TABLE task_outreach_targets
    DROP CONSTRAINT task_outreach_targets_response_message_id_fkey,
    ADD COLUMN outbox_id UUID REFERENCES email_outbox(id) ON DELETE SET NULL,
    ADD CONSTRAINT task_outreach_targets_response_message_id_fkey
        FOREIGN KEY (response_message_id) REFERENCES thread_messages(id) ON DELETE SET NULL,
    ADD CONSTRAINT task_outreach_targets_response_check CHECK (
        response_message_id IS NULL OR responded_at IS NOT NULL
    );

CREATE UNIQUE INDEX task_outreach_targets_outbox_idx
    ON task_outreach_targets (outbox_id) WHERE outbox_id IS NOT NULL;
CREATE INDEX task_outreaches_task_status_idx
    ON task_outreaches (task_id, status);
