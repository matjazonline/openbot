-- Complete legacy isolation and the104000 exact-association gate are independently reviewed.
-- Jobs/attempts remain the shared owners; this does not introduce an admission or runtime writer.
ALTER TABLE background_tasks
    DROP CONSTRAINT background_tasks_workflow_disabled,
    ALTER COLUMN channel_id DROP NOT NULL;
