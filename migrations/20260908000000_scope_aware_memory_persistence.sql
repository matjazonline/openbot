ALTER TABLE channels
    ADD COLUMN memory_persistence_mode TEXT NOT NULL DEFAULT 'audience_only'
        CHECK (memory_persistence_mode IN ('audience_only', 'scope_specific_facts'));
