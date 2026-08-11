-- up
ALTER TABLE workflows ADD COLUMN IF NOT EXISTS agent_ids UUID[];
