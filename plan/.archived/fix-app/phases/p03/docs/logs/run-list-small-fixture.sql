BEGIN;

\pset pager off
SET client_min_messages = warning;
CREATE TEMP TABLE workflow_versions (LIKE public.workflow_versions INCLUDING ALL);
CREATE TEMP TABLE workflow_runs (LIKE public.workflow_runs INCLUDING ALL);
-- 100 tenants x 40 workflows x 20 published versions. Agent k of tenant t is md5('agent'||t||'-'||k).
-- Agent 0 is dense (every even version of every workflow); agent 63 is sparse (workflow 0's first
-- version only); every version also uses one of agents 1..62.
INSERT INTO workflow_versions
    (company_id, workflow_id, id, draft_revision, command_key, actor_id, title, description, source,
     bundle, content_hash, created_at, agent_ids)
SELECT md5('company' || t)::uuid, md5('workflow' || t || '-' || w)::uuid,
       md5('version' || t || '-' || w || '-' || v)::uuid, v + 1, 'publish-' || w || '-' || v,
       md5('actor' || t)::uuid, 'Plan fixture', '', '{}', '\x00'::bytea, repeat('0', 64),
       now() - interval '2 years' + v * interval '36 days',
       array_remove(ARRAY[
           CASE WHEN v % 2 = 0 THEN md5('agent' || t || '-0')::uuid END,
           CASE WHEN w = 0 AND v = 0 THEN md5('agent' || t || '-63')::uuid END,
           md5('agent' || t || '-' || (1 + (w * 20 + v) % 62))::uuid], NULL)
FROM generate_series(1, 2) AS tenants(t), generate_series(0, 39) AS workflows(w),
     generate_series(0, 19) AS versions(v);
-- Four heavy tenants with 250,000 runs each and 96 light tenants with 1,000, spread over two
-- years. Run n of a tenant with N runs belongs to workflow n % 40 and, as time passes, to that
-- workflow's later versions (version 20n/N), so the sparse agent's runs are the oldest. About 95%
-- are terminal, finished ten minutes after they were created, and 10% are child runs, all through
-- history, so every board window holds children. Workflows 0..31 are bound to channels 0..31 of
-- their tenant (md5('channel' || t || '-' || w)); workflows 32..39 to the company.
INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, waiting_reason, parent_run_id, parent_execution_id, channel_id, finished_at, terminal_execution_id)
SELECT company, md5('run' || t || '-' || n)::uuid, md5('binding' || t || '-' || w)::uuid, 1,
       md5('workflow' || t || '-' || w)::uuid, md5('version' || t || '-' || w || '-' || v)::uuid,
       md5('actor' || t)::uuid, md5('trigger' || t || '-' || n)::uuid,
       md5('correlation' || t || '-' || n)::uuid, '\x00'::bytea, '{}', '{}', '{}', 100, 65536,
       created + interval '1 day', created, state,
       CASE WHEN state = 'waiting' THEN 'decision' END,
       CASE WHEN n % 10 = 5 THEN md5('run' || t || '-' || (n - 1))::uuid END,
       CASE WHEN n % 10 = 5 THEN md5('execution' || t || '-' || n)::uuid END,
       CASE WHEN w < 32 THEN md5('channel' || t || '-' || w)::uuid END,
       CASE WHEN state IN ('succeeded', 'failed', 'cancelled')
            THEN created + interval '10 minutes' END,
       CASE WHEN state IN ('succeeded','failed') THEN md5('terminal' || t || '-' || n)::uuid END
FROM (SELECT t, n, n % 40 AS w, 20 * n / total AS v, md5('company' || t)::uuid AS company,
             now() - interval '2 years' + (n::double precision / total) * interval '2 years' AS created,
             CASE WHEN n % 20 = 0 THEN (ARRAY['queued', 'running', 'waiting'])[1 + (n / 20) % 3]
                  ELSE (ARRAY['succeeded', 'failed', 'cancelled'])[1 + n % 3] END AS state
      FROM (SELECT t, 40 AS total
            FROM generate_series(1, 2) AS tenants(t)) AS tenant,
           generate_series(0, total - 1) AS runs(n)) AS seed;
ANALYZE workflow_versions;
ANALYZE workflow_runs;
\echo '== Table and index sizes (temporary clones)'
SELECT class.relname, class.reltuples::bigint AS rows, pg_size_pretty(pg_relation_size(class.oid)) AS size
  FROM pg_class AS class
 WHERE class.relpersistence = 't' AND class.relkind IN ('r', 'i')
 ORDER BY class.relkind DESC, class.relname;
\echo '== Shipped indexes on public.workflow_runs and public.workflow_versions'
SELECT indexdef FROM pg_indexes
 WHERE schemaname = 'public' AND tablename IN ('workflow_runs', 'workflow_versions') ORDER BY indexdef;

INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, parent_run_id, parent_execution_id, channel_id, finished_at, terminal_execution_id)
SELECT md5('company1')::uuid, md5('extra-run1-' || n)::uuid, md5('binding1-5')::uuid, 1,
       md5('workflow1-5')::uuid, md5('version1-5-18')::uuid, md5('actor1')::uuid,
       md5('extra-trigger1-' || n)::uuid, md5('extra-correlation1-' || n)::uuid, '\x00'::bytea,
       '{}', '{}', '{}', 100, 65536, created + interval '1 day', created, 'succeeded',
       md5('run1-4')::uuid, md5('extra-execution1-' || n)::uuid, md5('channel1-5')::uuid,
       created + interval '10 minutes',md5('extra-terminal1-' || n)::uuid
FROM (SELECT n, now() - interval '30 days' + n * interval '1 second' AS created
      FROM generate_series(1, 40) AS extra(n)) AS seed;
ANALYZE workflow_runs;
SET plan_cache_mode = force_generic_plan;

SELECT 'p03_rows='||count(*) FROM workflow_runs;
SELECT 'p03_bad_terminal_shape='||count(*) FROM workflow_runs WHERE (state IN ('succeeded','failed')) IS DISTINCT FROM (terminal_execution_id IS NOT NULL);
ROLLBACK;
