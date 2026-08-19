-- A reversible off switch for a channel. Disabling stops the channel taking traffic without
-- deleting its threads, tasks and approvals the way DELETE FROM channels does.
ALTER TABLE channels
    ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE;
