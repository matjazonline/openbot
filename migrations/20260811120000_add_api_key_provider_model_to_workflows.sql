-- Add api_key, provider, and model columns to workflows
ALTER TABLE workflows ADD COLUMN IF NOT EXISTS api_key VARCHAR(255);
ALTER TABLE workflows ADD COLUMN IF NOT EXISTS provider VARCHAR(100);
ALTER TABLE workflows ADD COLUMN IF NOT EXISTS model VARCHAR(100);
