-- Audience is a maximum disclosure boundary. Reclassification between fail-closed states is
-- allowed, as is narrowing an already-external row, but private or unknown content can never be
-- promoted into the external conversation in place.
CREATE FUNCTION prevent_message_audience_widening() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.audience <> 'external_conversation'
       AND NEW.audience = 'external_conversation' THEN
        RAISE EXCEPTION 'message audience cannot be widened in place'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER messages_audience_no_widen
BEFORE UPDATE OF audience ON messages
FOR EACH ROW EXECUTE FUNCTION prevent_message_audience_widening();

-- A draft version's workflow status may advance, but the version itself is an immutable artifact.
CREATE FUNCTION enforce_response_draft_update() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.id, OLD.version, OLD.company_id, OLD.channel_id, OLD.thread_id,
        OLD.task_id, OLD.author_principal_id, OLD.subject, OLD.body,
        OLD.recipient_snapshot) IS DISTINCT FROM
       (NEW.id, NEW.version, NEW.company_id, NEW.channel_id, NEW.thread_id,
        NEW.task_id, NEW.author_principal_id, NEW.subject, NEW.body,
        NEW.recipient_snapshot) THEN
        RAISE EXCEPTION 'response draft versions are immutable'
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

CREATE TRIGGER response_drafts_immutable_versions
BEFORE UPDATE ON response_drafts
FOR EACH ROW EXECUTE FUNCTION enforce_response_draft_update();
