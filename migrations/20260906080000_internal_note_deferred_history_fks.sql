-- Preserve note and instruction audit history when an individual referenced row is removed,
-- while allowing a whole company cascade to delete both sides in one transaction. RESTRICT is
-- checked before all company-owned cascades have run; deferred NO ACTION enforces the same
-- relationship at commit, after the aggregate has been removed.
ALTER TABLE internal_notes
    DROP CONSTRAINT internal_notes_message_fk,
    DROP CONSTRAINT internal_notes_author_fk,
    ADD CONSTRAINT internal_notes_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT internal_notes_author_fk
        FOREIGN KEY (company_id, author_principal_id)
        REFERENCES principals(company_id, id)
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE internal_note_tombstones
    DROP CONSTRAINT internal_note_tombstones_actor_fk,
    ADD CONSTRAINT internal_note_tombstones_actor_fk
        FOREIGN KEY (company_id, actor_principal_id)
        REFERENCES principals(company_id, id)
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE task_agent_instructions
    DROP CONSTRAINT task_agent_instructions_actor_fk,
    ADD CONSTRAINT task_agent_instructions_actor_fk
        FOREIGN KEY (company_id, requested_by_principal_id)
        REFERENCES principals(company_id, id)
        DEFERRABLE INITIALLY DEFERRED;
