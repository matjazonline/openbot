-- Keep the new phase constraint compatible with workers from the preceding application release
-- during a rolling deploy. New workers write phase explicitly; this trigger fills only legacy
-- status-only transitions.
CREATE FUNCTION synchronize_legacy_memory_provisioning_phase() RETURNS trigger AS $$
BEGIN
    IF NEW.status = 'completed' AND NEW.phase NOT IN ('ready', 'failed') THEN
        NEW.phase = 'ready';
    ELSIF NEW.status = 'failed' AND NEW.phase <> 'failed' THEN
        NEW.phase = 'failed';
    ELSIF NEW.status = 'pending' AND OLD.status IN ('completed', 'failed')
            AND NEW.phase IN ('ready', 'failed') THEN
        NEW.phase = 'create_pending';
        NEW.failure_attempts = 0;
        NEW.readiness_deadline = NULL;
        NEW.next_poll_at = NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER memory_provisioning_phase_compatibility_update
BEFORE UPDATE ON memory_provisioning_jobs
FOR EACH ROW EXECUTE FUNCTION synchronize_legacy_memory_provisioning_phase();
