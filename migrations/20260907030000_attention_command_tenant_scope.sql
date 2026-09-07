-- Client-chosen handoff ids and command ids are unique only inside their tenant. Keep both the
-- idempotency lookup and its database lock tenant-scoped so a colliding UUID in another company
-- can neither suppress a command nor disclose its resulting version.
ALTER TABLE attention_source_events
    DROP CONSTRAINT attention_source_events_source_command_key,
    ADD CONSTRAINT attention_source_events_source_command_key
        UNIQUE (company_id, source_kind, source_id, command_id);
