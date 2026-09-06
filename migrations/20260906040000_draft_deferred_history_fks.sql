-- Keep draft history when an individual referenced row is deleted, while allowing a whole
-- company cascade to remove both sides in one transaction. RESTRICT checks too early during that
-- cascade; deferred NO ACTION checks the same invariant after all cascades have run.
ALTER TABLE response_drafts
    DROP CONSTRAINT response_drafts_author_fk,
    ADD CONSTRAINT response_drafts_author_fk
        FOREIGN KEY (company_id, author_principal_id)
        REFERENCES principals(company_id, id)
        DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE response_draft_publications
    DROP CONSTRAINT response_draft_publications_draft_fk,
    DROP CONSTRAINT response_draft_publications_message_fk,
    DROP CONSTRAINT response_draft_publications_delivery_fk,
    DROP CONSTRAINT response_draft_publications_publisher_fk,
    ADD CONSTRAINT response_draft_publications_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT response_draft_publications_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT response_draft_publications_delivery_fk
        FOREIGN KEY (company_id, delivery_id)
        REFERENCES message_deliveries(company_id, id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT response_draft_publications_publisher_fk
        FOREIGN KEY (company_id, published_by_principal_id)
        REFERENCES principals(company_id, id)
        DEFERRABLE INITIALLY DEFERRED;
