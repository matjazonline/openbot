ALTER TABLE companies
    ADD COLUMN default_add_3rd_party BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN default_participant_emails CITEXT[],
    ADD COLUMN default_retrieve_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN default_retrieve_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN default_retrieve_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN default_persist_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN default_persist_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN default_persist_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD CONSTRAINT companies_default_participants_bounded CHECK (
        default_participant_emails IS NULL
        OR (
            cardinality(default_participant_emails) <= 64
            AND array_position(default_participant_emails, NULL) IS NULL
            AND array_position(default_participant_emails, ''::citext) IS NULL
        )
    );

ALTER TABLE agents
    ADD COLUMN memory_enabled BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE agent_channel_provisions
    ADD COLUMN warnings JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE channels
    ADD COLUMN owner_agent_id UUID,
    ADD CONSTRAINT channels_owner_agent_key UNIQUE (owner_agent_id),
    ADD CONSTRAINT channels_owner_agent_fk
        FOREIGN KEY (company_id, owner_agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE;

CREATE FUNCTION enforce_owned_channel_position_zero() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    checked_channel_id UUID;
BEGIN
    IF TG_TABLE_NAME = 'channels' THEN
        checked_channel_id := COALESCE(NEW.id, OLD.id);
    ELSE
        checked_channel_id := COALESCE(NEW.channel_id, OLD.channel_id);
    END IF;

    IF EXISTS (
        SELECT 1
        FROM channels AS channel
        WHERE channel.id = checked_channel_id
          AND channel.owner_agent_id IS NOT NULL
          AND NOT EXISTS (
              SELECT 1
              FROM channel_agents AS assignment
              WHERE assignment.channel_id = channel.id
                AND assignment.agent_id = channel.owner_agent_id
                AND assignment.position = 0
          )
    ) THEN
        RAISE EXCEPTION 'owned channel must assign its owner agent at position 0'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER owned_channel_position_zero_check
AFTER INSERT OR UPDATE OF owner_agent_id ON channels
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_owned_channel_position_zero();

CREATE CONSTRAINT TRIGGER owned_channel_assignment_position_zero_check
AFTER INSERT OR UPDATE OR DELETE ON channel_agents
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_owned_channel_position_zero();

CREATE FUNCTION prevent_owned_channel_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.owner_agent_id IS NOT NULL
       AND EXISTS (SELECT 1 FROM agents WHERE id = OLD.owner_agent_id) THEN
        RAISE EXCEPTION 'owned channel must be deleted through its owner agent'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER owned_channel_delete_guard
BEFORE DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION prevent_owned_channel_delete();
