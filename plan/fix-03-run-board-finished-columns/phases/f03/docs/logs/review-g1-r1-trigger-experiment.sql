-- F03-G1-R1 reviewer experiment (disposable database mail_agents_f03_b, freshly migrated from
-- migrations/20260817000000_init_schema.sql at 3793244). Applies G1-D1 revision 1 as written
-- (column, CHECK, trigger workflow_run_finished, function stamp_workflow_run_finished) to
-- public.workflow_runs, then exercises it on a clone that carries the same-named BEFORE UPDATE
-- triggers whose functions read the whole row (revision, snapshot guard), so firing order and
-- outcomes match the real table without the FK-laden fixture.
\set ON_ERROR_STOP 0
\pset pager off
SET client_min_messages = warning;

\echo '== B0 baseline at 3793244 (no finished_at): a state change in an UPDATE that also names a snapshot column at its own value'
CREATE TABLE baseline_runs (LIKE public.workflow_runs INCLUDING ALL);
CREATE TRIGGER workflow_run_revision BEFORE UPDATE ON baseline_runs
    FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_run_revision();
CREATE TRIGGER workflow_run_snapshot_immutable BEFORE UPDATE OF company_id, id, binding_id,
    binding_revision, workflow_id, version_id, channel_id, thread_id, actor_id, trigger_id,
    correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
    created_at, parent_run_id, parent_execution_id, rerun_of_run_id, delivery_policy
    ON baseline_runs FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_run_snapshot();
INSERT INTO baseline_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id,
    actor_id, trigger_id, correlation_id, bundle, input, params, resources, max_steps,
    max_context_bytes, deadline, state)
VALUES (md5('c')::uuid, md5('r0')::uuid, md5('b')::uuid, 1, md5('w')::uuid, md5('v')::uuid,
        md5('a')::uuid, md5('t0')::uuid, md5('k0')::uuid, '\x00'::bytea, '{}', '{}', '{}', 10, 1024,
        now() + interval '1 day', 'running');
UPDATE baseline_runs SET state = 'succeeded', deadline = deadline WHERE id = md5('r0')::uuid
    RETURNING state, revision;
DROP TABLE baseline_runs;

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

\echo '== E0 BEFORE row triggers on public.workflow_runs in firing (name) order, after G1-D1'
SELECT tgname, (tgtype & 4) = 4 AS on_insert, (tgtype & 16) = 16 AS on_update,
       pg_get_triggerdef(oid) LIKE '% UPDATE OF %' AS column_list
  FROM pg_trigger WHERE tgrelid = 'public.workflow_runs'::regclass AND NOT tgisinternal
   AND (tgtype & 2) = 2 ORDER BY tgname;

\echo '== E0b capture_catalog query (action_reconciliation_claim_sql_candidate_support.rs:104) now returns'
SELECT tgname::text FROM pg_trigger WHERE tgrelid='public.workflow_runs'::regclass AND NOT tgisinternal
   AND (tgtype & 2)=2 AND (tgtype & 16)=16 ORDER BY tgname;

-- Clone with the same constraints; attach the row-reading BEFORE UPDATE triggers by their real names.
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
FROM generate_series(1, 4) AS runs(n);

\echo '== E1 running -> failed stamps, revision 1 -> 2'
UPDATE scratch_runs SET state = 'failed' WHERE id = md5('r1')::uuid
    RETURNING state, finished_at IS NOT NULL AS stamped, revision;
\echo '== E2 failed -> failed with finished_at = NULL keeps the stamp and does not bump revision'
CREATE TEMP TABLE kept AS SELECT finished_at FROM scratch_runs WHERE id = md5('r1')::uuid;
UPDATE scratch_runs SET state = 'failed', finished_at = NULL WHERE id = md5('r1')::uuid
    RETURNING state, finished_at = (SELECT finished_at FROM kept) AS kept_first_finish, revision;
\echo '== E3 retry: failed -> running clears it'
UPDATE scratch_runs SET state = 'running' WHERE id = md5('r1')::uuid
    RETURNING state, finished_at, revision;
\echo '== E4 running -> succeeded in an UPDATE that also names a snapshot column at its own value'
UPDATE scratch_runs SET state = 'succeeded', deadline = deadline WHERE id = md5('r2')::uuid
    RETURNING state, finished_at IS NOT NULL AS stamped;
\echo '== E5 a direct write of finished_at alone (no state in SET): allowed on a terminal row'
UPDATE scratch_runs SET state = 'cancelled' WHERE id = md5('r3')::uuid RETURNING state, revision;
UPDATE scratch_runs SET finished_at = '2000-01-01' WHERE id = md5('r3')::uuid
    RETURNING state, finished_at, revision;
\echo '== E5b ... and rejected by the CHECK on an open row'
UPDATE scratch_runs SET finished_at = now() WHERE id = md5('r4')::uuid RETURNING state, finished_at;
\echo '== E6 INSERT straight into a terminal state stamps; a supplied finished_at is overwritten'
INSERT INTO scratch_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id,
    actor_id, trigger_id, correlation_id, bundle, input, params, resources, max_steps,
    max_context_bytes, deadline, state, finished_at)
VALUES (md5('c')::uuid, md5('r5')::uuid, md5('b')::uuid, 1, md5('w')::uuid, md5('v')::uuid,
        md5('a')::uuid, md5('t5')::uuid, md5('k5')::uuid, '\x00'::bytea, '{}', '{}', '{}', 10, 1024,
        now() + interval '1 day', 'failed', '2000-01-01')
RETURNING state, finished_at > '2001-01-01' AS stamped_now;

\echo '== E7 ordering dependency: the same stamp under a name that sorts after workflow_run_revision'
DROP TRIGGER workflow_run_finished ON scratch_runs;
CREATE TRIGGER workflow_zz_finished BEFORE INSERT OR UPDATE OF state ON scratch_runs
    FOR EACH ROW EXECUTE FUNCTION public.stamp_workflow_run_finished();
UPDATE scratch_runs SET state = 'failed' WHERE id = md5('r4')::uuid RETURNING state, revision;
CREATE TEMP TABLE before_e7 AS SELECT revision, finished_at FROM scratch_runs WHERE id = md5('r4')::uuid;
UPDATE scratch_runs SET state = 'failed', finished_at = NULL WHERE id = md5('r4')::uuid
    RETURNING state, finished_at = (SELECT finished_at FROM before_e7) AS kept_first_finish,
              revision AS revision_after, (SELECT revision FROM before_e7) AS revision_before;
DROP TABLE scratch_runs;
