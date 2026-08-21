-- Company deletion must continue cascading through its channels and agents. Only global library
-- definitions need an in-use deletion guard.
ALTER TABLE channel_agents DROP CONSTRAINT channel_agents_agent_fk;
ALTER TABLE channel_agents
    ADD CONSTRAINT channel_agents_agent_fk
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE;

CREATE FUNCTION prevent_assigned_library_agent_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM channel_agents WHERE agent_id = OLD.id) THEN
        RAISE EXCEPTION 'library agent is assigned to one or more channels'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER library_agent_delete_guard
BEFORE DELETE ON agents
FOR EACH ROW EXECUTE FUNCTION prevent_assigned_library_agent_delete();
