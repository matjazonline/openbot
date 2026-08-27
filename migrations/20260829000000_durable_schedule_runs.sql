CREATE TABLE schedule_runs (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL REFERENCES channel_schedules(id) ON DELETE CASCADE,
    scheduled_for TIMESTAMPTZ NOT NULL,
    schedule_snapshot JSONB NOT NULL,
    thread_id UUID REFERENCES threads(id) ON DELETE SET NULL,
    task_id UUID REFERENCES background_tasks(id) ON DELETE SET NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT schedule_runs_schedule_slot_key UNIQUE (schedule_id, scheduled_for),
    CONSTRAINT schedule_runs_snapshot_object_check
        CHECK (jsonb_typeof(schedule_snapshot) = 'object'),
    CONSTRAINT schedule_runs_task_requires_thread_check
        CHECK (task_id IS NULL OR thread_id IS NOT NULL)
);

CREATE INDEX schedule_runs_pending_idx
    ON schedule_runs (created_at, id)
    WHERE task_id IS NULL;

CREATE INDEX schedule_runs_schedule_created_idx
    ON schedule_runs (schedule_id, created_at DESC, id DESC);
