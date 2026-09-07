-- Ownership is an attention-visible change even though it has an independent command fence.
-- Keep the projection version monotonic for every ownership writer, including principal-removal
-- cleanup inside PostgreSQL, without coupling worker leases to business-attribute commands.
CREATE FUNCTION bump_task_attention_version_for_owner_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.owner_principal_id, NEW.owner_principal_kind)
       IS DISTINCT FROM (OLD.owner_principal_id, OLD.owner_principal_kind) THEN
        NEW.attention_version := OLD.attention_version + 1;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER background_tasks_bump_attention_version
BEFORE UPDATE OF owner_principal_id, owner_principal_kind ON background_tasks
FOR EACH ROW EXECUTE FUNCTION bump_task_attention_version_for_owner_change();
