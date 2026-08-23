DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM channel_schedules AS schedule
        JOIN channels AS channel ON channel.id = schedule.channel_id
        WHERE schedule.company_id <> channel.company_id
    ) THEN
        RAISE EXCEPTION
            'channel_schedules contains company/channel tenant mismatches; repair rows before migrating';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM thread_messages AS message
        JOIN threads AS thread ON thread.id = message.thread_id
        WHERE message.company_id <> thread.company_id
           OR message.channel_id <> thread.channel_id
    ) THEN
        RAISE EXCEPTION
            'thread_messages contains company/channel/thread tenant mismatches; repair rows before migrating';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM thread_messages AS message
        JOIN email_messages AS email ON email.id = message.email_message_id
        WHERE message.company_id <> email.company_id
    ) THEN
        RAISE EXCEPTION
            'thread_messages contains company/email tenant mismatches; repair rows before migrating';
    END IF;
END
$$;

ALTER TABLE channel_schedules
    DROP CONSTRAINT channel_schedules_channel_id_fkey,
    ADD CONSTRAINT channel_schedules_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id)
        ON DELETE CASCADE
        NOT VALID;

ALTER TABLE thread_messages
    DROP CONSTRAINT thread_messages_thread_fk,
    ADD CONSTRAINT thread_messages_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id)
        ON DELETE CASCADE
        NOT VALID;

ALTER TABLE channel_schedules
    VALIDATE CONSTRAINT channel_schedules_channel_fk;

ALTER TABLE thread_messages
    VALIDATE CONSTRAINT thread_messages_thread_fk;
