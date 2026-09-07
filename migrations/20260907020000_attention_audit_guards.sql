ALTER TABLE attention_source_events
    ADD CONSTRAINT attention_source_events_company_fk
        FOREIGN KEY (company_id) REFERENCES companies(id) ON DELETE CASCADE;

CREATE FUNCTION attention_source_events_are_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND NOT EXISTS (SELECT 1 FROM companies WHERE id = OLD.company_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'attention source events are immutable' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER attention_source_events_immutable
BEFORE UPDATE OR DELETE ON attention_source_events
FOR EACH ROW EXECUTE FUNCTION attention_source_events_are_immutable();

-- Removing a teammate releases their open handoffs just as task ownership cleanup releases tasks.
-- Authorship and resolution remain in the immutable event log, so they must not prevent account
-- removal or leave an unavailable principal responsible for current work.
ALTER TABLE manual_handoffs
    DROP CONSTRAINT manual_handoffs_responsible_fk,
    ADD CONSTRAINT manual_handoffs_responsible_fk
        FOREIGN KEY (company_id, responsible_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (responsible_principal_id),
    DROP CONSTRAINT manual_handoffs_creator_fk,
    ALTER COLUMN created_by_principal_id DROP NOT NULL,
    ADD CONSTRAINT manual_handoffs_creator_fk
        FOREIGN KEY (company_id, created_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (created_by_principal_id),
    DROP CONSTRAINT manual_handoffs_resolver_fk,
    ADD CONSTRAINT manual_handoffs_resolver_fk
        FOREIGN KEY (company_id, resolved_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (resolved_by_principal_id),
    DROP CONSTRAINT manual_handoffs_resolution_check,
    ADD CONSTRAINT manual_handoffs_resolution_check CHECK (
        (status = 'open' AND resolved_at IS NULL AND resolved_by_principal_id IS NULL)
        OR (status <> 'open' AND resolved_at IS NOT NULL)
    );

ALTER TABLE attention_source_events
    DROP CONSTRAINT attention_source_events_actor_fk,
    ALTER COLUMN actor_principal_id DROP NOT NULL,
    ADD CONSTRAINT attention_source_events_actor_fk
        FOREIGN KEY (company_id, actor_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (actor_principal_id),
    DROP CONSTRAINT attention_source_events_previous_responsible_fk,
    ADD CONSTRAINT attention_source_events_previous_responsible_fk
        FOREIGN KEY (company_id, previous_responsible_principal_id)
        REFERENCES principals(company_id, id)
        ON DELETE SET NULL (previous_responsible_principal_id),
    DROP CONSTRAINT attention_source_events_new_responsible_fk,
    ADD CONSTRAINT attention_source_events_new_responsible_fk
        FOREIGN KEY (company_id, new_responsible_principal_id)
        REFERENCES principals(company_id, id)
        ON DELETE SET NULL (new_responsible_principal_id);

CREATE FUNCTION bump_handoff_version_for_responsibility_cleanup() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.responsible_principal_id IS DISTINCT FROM OLD.responsible_principal_id
       AND NEW.version = OLD.version THEN
        NEW.version := OLD.version + 1;
        NEW.updated_at := CURRENT_TIMESTAMP;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER manual_handoffs_bump_cleanup_version
BEFORE UPDATE OF responsible_principal_id ON manual_handoffs
FOR EACH ROW EXECUTE FUNCTION bump_handoff_version_for_responsibility_cleanup();
