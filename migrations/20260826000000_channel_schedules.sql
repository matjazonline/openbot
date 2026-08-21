CREATE TABLE channel_schedules (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    schedule_type TEXT NOT NULL,
    interval_seconds BIGINT,
    subject_template TEXT NOT NULL,
    prompt_template TEXT NOT NULL,
    delivery_mode TEXT NOT NULL DEFAULT 'mailbox_only',
    recipient_emails CITEXT[] NOT NULL DEFAULT '{}',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    enabled BOOLEAN NOT NULL DEFAULT true,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT channel_schedules_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT channel_schedules_type_check CHECK (schedule_type IN ('interval', 'one_off')),
    CONSTRAINT channel_schedules_interval_check CHECK (
        (schedule_type = 'interval' AND interval_seconds IS NOT NULL AND interval_seconds >= 60)
        OR (schedule_type = 'one_off' AND interval_seconds IS NULL)
    ),
    CONSTRAINT channel_schedules_delivery_mode_check CHECK (
        delivery_mode IN ('mailbox_only', 'email_participants', 'email_custom')
    ),
    -- A schedule renders its templates and counts its days in this zone, so an unknown name has to
    -- be refused at write time: the claim query would otherwise fail on every tick.
    CONSTRAINT channel_schedules_timezone_check CHECK (now() AT TIME ZONE timezone IS NOT NULL)
);

CREATE INDEX channel_schedules_due_idx
    ON channel_schedules (next_run_at, id)
    WHERE enabled = true AND next_run_at IS NOT NULL;

CREATE INDEX channel_schedules_company_idx
    ON channel_schedules (company_id, created_at DESC, id DESC);

CREATE INDEX channel_schedules_channel_idx
    ON channel_schedules (channel_id, created_at DESC, id DESC);

-- The runs column filters background_tasks by the schedule id inside the payload. Without this the
-- lookup scans every task ever queued, scheduled or not.
CREATE INDEX background_tasks_schedule_idx
    ON background_tasks ((payload->>'schedule_id'))
    WHERE task_type = 'scheduled_agent_run';
