-- Note audit rows live for the lifetime of their thread. When that aggregate is deliberately
-- removed, correction links and task selections must not make the thread undeletable.
ALTER TABLE internal_notes
    DROP CONSTRAINT internal_notes_supersedes_fk,
    ADD CONSTRAINT internal_notes_supersedes_fk
        FOREIGN KEY (company_id, supersedes_note_id)
        REFERENCES internal_notes(company_id, id) ON DELETE CASCADE;

ALTER TABLE task_agent_instruction_notes
    DROP CONSTRAINT task_agent_instruction_notes_note_fk,
    ADD CONSTRAINT task_agent_instruction_notes_note_fk
        FOREIGN KEY (company_id, note_id)
        REFERENCES internal_notes(company_id, id) ON DELETE CASCADE;
