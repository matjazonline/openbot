-- A publication is history owned by its draft. Removing a whole company cascades through the
-- channel to the draft, so its publication record must follow; individual draft deletion remains
-- impossible while the ordinary product exposes no such operation.
ALTER TABLE response_draft_publications
    DROP CONSTRAINT response_draft_publications_draft_fk,
    ADD CONSTRAINT response_draft_publications_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED;
