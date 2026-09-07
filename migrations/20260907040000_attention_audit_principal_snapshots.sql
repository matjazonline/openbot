-- Principal ids in an immutable audit row are historical snapshots, like the ids in
-- task_ownership_events. Foreign-key SET NULL would rewrite the event and is correctly rejected
-- by the immutability trigger, while RESTRICT would prevent normal teammate removal.
ALTER TABLE attention_source_events
    DROP CONSTRAINT attention_source_events_actor_fk,
    DROP CONSTRAINT attention_source_events_previous_responsible_fk,
    DROP CONSTRAINT attention_source_events_new_responsible_fk,
    ALTER COLUMN actor_principal_id SET NOT NULL;
