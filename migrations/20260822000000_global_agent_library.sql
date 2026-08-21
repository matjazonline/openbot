-- Agents with no company belong to the operator-managed global library.
ALTER TABLE agents ALTER COLUMN company_id DROP NOT NULL;

CREATE UNIQUE INDEX agents_library_slug_key
    ON agents (slug) WHERE company_id IS NULL;

ALTER TABLE channel_agents DROP CONSTRAINT channel_agents_agent_fk;
ALTER TABLE channel_agents
    ADD CONSTRAINT channel_agents_agent_fk
    FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE RESTRICT;

CREATE FUNCTION enforce_channel_agent_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND (agent.company_id IS NULL OR agent.company_id = NEW.company_id)
    ) THEN
        RAISE EXCEPTION 'agent must belong to the channel company or the global library'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER channel_agents_scope_check
BEFORE INSERT OR UPDATE ON channel_agents
FOR EACH ROW EXECUTE FUNCTION enforce_channel_agent_scope();
