-- Transaction-local copies with production indexes and 50,000 runs / 100 threads.
-- No production rows or schema are changed.
BEGIN;
CREATE TEMP TABLE workflow_runs (LIKE public.workflow_runs INCLUDING ALL);
CREATE TEMP TABLE workflow_versions (LIKE public.workflow_versions INCLUDING ALL);
INSERT INTO workflow_versions SELECT v.* FROM public.workflow_versions v WHERE id = (SELECT version_id FROM public.workflow_runs LIMIT 1);
WITH exemplar AS (SELECT to_jsonb(r) AS row FROM public.workflow_runs r LIMIT 1)
INSERT INTO workflow_runs
SELECT seeded.* FROM exemplar, generate_series(1,50000) i
CROSS JOIN LATERAL jsonb_populate_record(NULL::pg_temp.workflow_runs, row || jsonb_build_object(
 'id', md5('p09-run-'||i)::uuid, 'channel_id', md5('p09-channel')::uuid, 'thread_id', md5('p09-thread-'||(i % 100))::uuid,
 'created_at', now() - i * interval '1 second', 'deadline', now() + interval '1 day',
 'state', CASE WHEN i % 3 = 0 THEN 'succeeded' ELSE 'queued' END, 'waiting_reason', NULL)) seeded;
ANALYZE workflow_runs;
ANALYZE workflow_versions;
EXPLAIN (ANALYZE, BUFFERS)
SELECT run.id AS run_id, version.title, run.state, run.waiting_reason
FROM workflow_runs run
JOIN workflow_versions version ON (version.company_id, version.id) = (run.company_id, run.version_id)
WHERE run.company_id = (SELECT company_id FROM workflow_versions LIMIT 1)
AND run.thread_id = md5('p09-thread-1')::uuid
AND run.state IN ('queued','running','waiting','failed')
ORDER BY run.created_at DESC,run.id DESC LIMIT 51;
ROLLBACK;
