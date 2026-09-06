-- A target id is only meaningful inside its outreach. Composite references prevent a valid target
-- from another outreach in the same tenant satisfying a reply, replacement or command relation.
ALTER TABLE task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_company_outreach_id_key
        UNIQUE (company_id, outreach_id, id),
    DROP CONSTRAINT task_outreach_targets_replacement_fk,
    ADD CONSTRAINT task_outreach_targets_replacement_fk
        FOREIGN KEY (company_id, outreach_id, replaces_target_id)
        REFERENCES task_outreach_targets(company_id, outreach_id, id) ON DELETE RESTRICT;

ALTER TABLE task_outreach_replies
    DROP CONSTRAINT task_outreach_replies_target_fk,
    ADD CONSTRAINT task_outreach_replies_target_fk
        FOREIGN KEY (company_id, outreach_id, target_id)
        REFERENCES task_outreach_targets(company_id, outreach_id, id) ON DELETE CASCADE;

ALTER TABLE delegation_control_commands
    DROP CONSTRAINT delegation_control_commands_target_fk,
    ADD CONSTRAINT delegation_control_commands_target_fk
        FOREIGN KEY (company_id, outreach_id, target_id)
        REFERENCES task_outreach_targets(company_id, outreach_id, id) ON DELETE RESTRICT;
