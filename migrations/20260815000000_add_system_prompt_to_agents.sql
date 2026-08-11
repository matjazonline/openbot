-- up
ALTER TABLE agents ADD COLUMN IF NOT EXISTS system_prompt TEXT;
