-- Announce task status changes so an open mailbox can show what an agent is doing.
--
-- `UPDATE OF status` is load-bearing: the worker renews a task's lease every few seconds while it
-- runs (`renew_task_lease` touches only `lock_expires_at`), and a trigger on any UPDATE would turn
-- every heartbeat of every running task into a broadcast to every connected mailbox.
--
-- Tasks with no thread -- and there are some, `thread_id` is nullable and a deleted thread nulls it
-- -- have nothing to display against, so they are skipped rather than published and filtered later.

CREATE OR REPLACE FUNCTION notify_thread_activity() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify(
        'thread_activity',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_notify_activity
    AFTER INSERT OR UPDATE OF status ON background_tasks
    FOR EACH ROW
    WHEN (NEW.thread_id IS NOT NULL)
    EXECUTE FUNCTION notify_thread_activity();
