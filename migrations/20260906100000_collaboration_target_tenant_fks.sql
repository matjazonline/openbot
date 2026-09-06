-- Every target-side relationship carries the company discriminator. A valid delivery, message or
-- thread-message id from another tenant must not satisfy a collaboration target reference.

ALTER TABLE thread_messages
    ADD CONSTRAINT thread_messages_company_id_id_key UNIQUE (company_id, id);

ALTER TABLE task_outreach_targets
    DROP CONSTRAINT task_outreach_targets_delivery_id_fkey,
    DROP CONSTRAINT task_outreach_targets_request_message_id_fkey,
    DROP CONSTRAINT task_outreach_targets_response_association_fk,
    ADD CONSTRAINT task_outreach_targets_delivery_fk
        FOREIGN KEY (company_id, delivery_id)
        REFERENCES message_deliveries(company_id, id) ON DELETE SET NULL (delivery_id),
    ADD CONSTRAINT task_outreach_targets_request_message_fk
        FOREIGN KEY (company_id, request_message_id)
        REFERENCES messages(company_id, id) ON DELETE SET NULL (request_message_id),
    ADD CONSTRAINT task_outreach_targets_response_association_fk
        FOREIGN KEY (company_id, response_association_id)
        REFERENCES thread_messages(company_id, id)
        ON DELETE SET NULL (response_association_id);
