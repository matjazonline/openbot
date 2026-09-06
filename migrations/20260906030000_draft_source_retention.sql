-- Retention may delete the optional task a draft came from. Preserve every authored field while
-- allowing that foreign key to be cleared; it may never be replaced with a different task.
CREATE OR REPLACE FUNCTION enforce_response_draft_update() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.id, OLD.version, OLD.company_id, OLD.channel_id, OLD.thread_id,
        OLD.author_principal_id, OLD.subject, OLD.body, OLD.recipient_snapshot) IS DISTINCT FROM
       (NEW.id, NEW.version, NEW.company_id, NEW.channel_id, NEW.thread_id,
        NEW.author_principal_id, NEW.subject, NEW.body, NEW.recipient_snapshot) THEN
        RAISE EXCEPTION 'response draft versions are immutable'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.task_id IS DISTINCT FROM NEW.task_id
       AND NOT (OLD.task_id IS NOT NULL AND NEW.task_id IS NULL) THEN
        RAISE EXCEPTION 'a response draft task source cannot be replaced'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.status <> NEW.status AND NOT (
        OLD.status = 'active' AND NEW.status IN ('superseded', 'published')
    ) THEN
        RAISE EXCEPTION 'invalid response draft status transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
