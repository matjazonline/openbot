-- A transport-authored message must name a handle owned by its stated author principal. The
-- former two-column foreign key proved only that both rows belonged to the company.
ALTER TABLE participant_identities
    ADD CONSTRAINT participant_identities_company_principal_id_key
    UNIQUE (company_id, principal_id, id);

ALTER TABLE messages
    ADD CONSTRAINT messages_authored_identity_author_fk
    FOREIGN KEY (company_id, author_principal_id, authored_identity_id)
    REFERENCES participant_identities(company_id, principal_id, id)
    ON DELETE SET NULL (authored_identity_id)
    NOT VALID;

ALTER TABLE messages VALIDATE CONSTRAINT messages_authored_identity_author_fk;

-- Direct changes to an audit row are forbidden. A parent lifecycle cascade is still allowed: the
-- foreign-key trigger calls this trigger at depth greater than one, whereas a direct statement
-- enters it at depth one.
CREATE OR REPLACE FUNCTION reject_binding_audit_rewrite() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' OR pg_trigger_depth() <= 1 THEN
        RAISE EXCEPTION 'binding_audit_events is append-only' USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END;
$$;

DROP TRIGGER binding_audit_events_append_only ON binding_audit_events;
CREATE TRIGGER binding_audit_events_append_only
BEFORE UPDATE OR DELETE ON binding_audit_events
FOR EACH ROW EXECUTE FUNCTION reject_binding_audit_rewrite();
