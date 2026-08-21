-- A short, human-written statement of what this agent is for.
--
-- Read by the `list_company_agents` tool so a calling agent can pick the right colleague without
-- having its address book hardcoded into a system prompt.
ALTER TABLE agents ADD COLUMN description TEXT;
