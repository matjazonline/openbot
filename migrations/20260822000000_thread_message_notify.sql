-- Announce every persisted message so open `/ui` mailboxes can append it live.
--
-- This lives in a trigger rather than in the Rust writer for three reasons: every writer is
-- covered, including ones added later; the notification is bound to the same transaction as the
-- row, so it is delivered only on commit and never announces a message a reader cannot yet see;
-- and `create_message` does not have `company_id` in hand -- it derives it in SQL from `threads`.
--
-- The payload carries identifiers only. `pg_notify` caps payloads at 8000 bytes, and listeners
-- re-query for the message body anyway so that a reader resuming after a dropped connection takes
-- the same path as one receiving a live message.

CREATE OR REPLACE FUNCTION notify_thread_message() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify(
        'thread_message',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- INSERT only: `create_message` upserts, but the UPDATE branch just rewrites the body of a message
-- that was already announced, and re-announcing it would append a duplicate bubble.
CREATE TRIGGER thread_messages_notify
    AFTER INSERT ON thread_messages
    FOR EACH ROW
    EXECUTE FUNCTION notify_thread_message();
