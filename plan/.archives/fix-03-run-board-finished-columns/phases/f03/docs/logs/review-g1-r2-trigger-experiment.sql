-- F03-G1-R2 reviewer recheck (disposable database mail_agents_f03_b, freshly migrated from the init
-- schema at 3793244). Applies G1-D1 revision 2: column, CHECK, trigger workflow_run_finished, and
-- finished_at added to guard_workflow_run_snapshot's exemption array on both sides. Rechecks
-- F03-G1-F1's reproductions on a clone carrying the same-named row-reading BEFORE UPDATE triggers.
\set ON_ERROR_STOP 0
\pset pager off
SET client_min_messages = warning;
ALTER TABLE public.workflow_runs ADD COLUMN finished_at timestamptz;
ALTER TABLE public.workflow_runs ADD CONSTRAINT workflow_run_finished_shape
    CHECK ((state IN ('succeeded','failed','cancelled')) = (finished_at IS NOT NULL));
CREATE FUNCTION public.stamp_workflow_run_finished() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.state IN ('succeeded','failed','cancelled') THEN
        IF TG_OP = 'UPDATE' AND OLD.state IN ('succeeded','failed','cancelled') THEN
            NEW.finished_at := OLD.finished_at;
        ELSE
            NEW.finished_at := clock_timestamp();
        END IF;
    ELSE
        NEW.finished_at := NULL;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_run_finished BEFORE INSERT OR UPDATE OF state ON public.workflow_runs
    FOR EACH ROW EXECUTE FUNCTION public.stamp_workflow_run_finished();
CREATE OR REPLACE FUNCTION public.guard_workflow_run_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW) - ARRAY['state','waiting_reason','terminal_execution_id',
        'terminal_wakeup_sequence','revision','finished_at']) IS DISTINCT FROM
       (to_jsonb(OLD) - ARRAY['state','waiting_reason','terminal_execution_id',
        'terminal_wakeup_sequence','revision','finished_at']) THEN
        RAISE EXCEPTION 'workflow run snapshot is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TABLE scratch_runs (LIKE public.workflow_runs INCLUDING ALL);
CREATE TRIGGER workflow_run_finished BEFORE INSERT OR UPDATE OF state ON scratch_runs
    FOR EACH ROW EXECUTE FUNCTION public.stamp_workflow_run_finished();
CREATE TRIGGER workflow_run_revision BEFORE UPDATE ON scratch_runs
    FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_run_revision();
CREATE TRIGGER workflow_run_snapshot_immutable BEFORE UPDATE OF company_id, id, binding_id,
    binding_revision, workflow_id, version_id, channel_id, thread_id, actor_id, trigger_id,
    correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
    created_at, parent_run_id, parent_execution_id, rerun_of_run_id, delivery_policy
    ON scratch_runs FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_run_snapshot();
INSERT INTO scratch_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id,
    actor_id, trigger_id, correlation_id, bundle, input, params, resources, max_steps,
    max_context_bytes, deadline, state)
SELECT md5('c')::uuid, md5('r' || n)::uuid, md5('b')::uuid, 1, md5('w')::uuid, md5('v')::uuid,
       md5('a')::uuid, md5('t' || n)::uuid, md5('k' || n)::uuid, '\x00'::bytea, '{}', '{}', '{}',
       10, 1024, now() + interval '1 day', 'running'
FROM generate_series(1, 3) AS runs(n);
\echo '== R2-E4 running -> succeeded beside a snapshot column at its own value (F1 reproduction): must succeed'
UPDATE scratch_runs SET state = 'succeeded', deadline = deadline WHERE id = md5('r1')::uuid
    RETURNING state, finished_at IS NOT NULL AS stamped, revision;
\echo '== R2-E4b a real snapshot change beside a state change is still refused'
UPDATE scratch_runs SET state = 'failed', deadline = deadline + interval '1 second' WHERE id = md5('r2')::uuid
    RETURNING state;
\echo '== R2-E2 same terminal with finished_at = NULL: stamp kept, revision unchanged'
UPDATE scratch_runs SET state = 'failed' WHERE id = md5('r3')::uuid RETURNING state, revision;
CREATE TEMP TABLE kept AS SELECT finished_at, revision FROM scratch_runs WHERE id = md5('r3')::uuid;
UPDATE scratch_runs SET state = 'failed', finished_at = NULL WHERE id = md5('r3')::uuid
    RETURNING finished_at = (SELECT finished_at FROM kept) AS kept_first_finish,
              revision = (SELECT revision FROM kept) AS revision_unchanged;
\echo '== R2-E0 trigger name order on public.workflow_runs (BEFORE UPDATE)'
SELECT tgname::text FROM pg_trigger WHERE tgrelid='public.workflow_runs'::regclass AND NOT tgisinternal
   AND (tgtype & 2)=2 AND (tgtype & 16)=16 ORDER BY tgname;
DROP TABLE scratch_runs;
