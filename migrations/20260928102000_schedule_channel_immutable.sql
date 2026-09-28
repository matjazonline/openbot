-- A schedule selects its channel at creation. An update cannot reassign it,
-- including a raw SQL update concurrent with occurrence capture.
CREATE FUNCTION public.require_immutable_schedule_channel() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.channel_id IS DISTINCT FROM OLD.channel_id THEN
        RAISE EXCEPTION 'A schedule cannot change its channel'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_channel_immutable';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER schedule_channel_immutable
    BEFORE UPDATE OF channel_id ON public.channel_schedules
    FOR EACH ROW EXECUTE FUNCTION public.require_immutable_schedule_channel();
