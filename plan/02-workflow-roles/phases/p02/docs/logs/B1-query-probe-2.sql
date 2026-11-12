SET statement_timeout='120s';

\pset pager off
SET client_min_messages = warning;
CREATE TEMP TABLE workflow_versions (LIKE public.workflow_versions INCLUDING ALL);
CREATE TEMP TABLE workflow_runs (LIKE public.workflow_runs INCLUDING ALL);
CREATE TEMP TABLE run_dependency_manifests (LIKE public.run_dependency_manifests INCLUDING ALL);
CREATE TEMP TABLE workflow_run_role_bindings (LIKE public.workflow_run_role_bindings INCLUDING ALL);
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
FROM generate_series(1, 100) AS tenants(t), generate_series(0, 39) AS workflows(w),
     generate_series(0, 19) AS versions(v);
-- Four heavy tenants with 250,000 runs each and 96 light tenants with 1,000, spread over two
-- years. Run n of a tenant with N runs belongs to workflow n % 40 and, as time passes, to that
-- workflow's later versions (version 20n/N), so the sparse agent's runs are the oldest. About 95%
-- are terminal, finished ten minutes after they were created, and 10% are child runs, all through
-- history, so every board window holds children. Workflows 0..31 are bound to channels 0..31 of
-- their tenant (md5('channel' || t || '-' || w)); workflows 32..39 to the company.
INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle_digest, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, waiting_reason, parent_run_id, parent_execution_id, channel_id, finished_at, terminal_execution_id, knowledge_ceiling, content_exact)
SELECT company, md5('run' || t || '-' || n)::uuid, md5('binding' || t || '-' || w)::uuid, 1,
       md5('workflow' || t || '-' || w)::uuid, md5('version' || t || '-' || w || '-' || v)::uuid,
       md5('actor' || t)::uuid, md5('trigger' || t || '-' || n)::uuid,
       md5('correlation' || t || '-' || n)::uuid, sha256('\x00'::bytea), '{}', '{}', '{}', 100, 65536,
       created + interval '1 day', created, state,
       CASE WHEN state = 'waiting' THEN 'decision' END,
       CASE WHEN n % 10 = 5 THEN md5('run' || t || '-' || (n - 1))::uuid END,
       CASE WHEN n % 10 = 5 THEN md5('execution' || t || '-' || n)::uuid END,
       CASE WHEN w < 32 THEN md5('channel' || t || '-' || w)::uuid END,
       CASE WHEN state IN ('succeeded', 'failed', 'cancelled')
            THEN created + interval '10 minutes' END,
       CASE WHEN state IN ('succeeded','failed') THEN md5('terminal' || t || '-' || n)::uuid END, 9, true
FROM (SELECT t, n, n % 40 AS w, 20 * n / total AS v, md5('company' || t)::uuid AS company,
             now() - interval '2 years' + (n::double precision / total) * interval '2 years' AS created,
             CASE WHEN n % 20 = 0 THEN (ARRAY['queued', 'running', 'waiting'])[1 + (n / 20) % 3]
                  ELSE (ARRAY['succeeded', 'failed', 'cancelled'])[1 + n % 3] END AS state
      FROM (SELECT t, CASE WHEN t <= 4 THEN 250000 ELSE 1000 END AS total
            FROM generate_series(1, 100) AS tenants(t)) AS tenant,
           generate_series(0, total - 1) AS runs(n)) AS seed;
-- Every run owns or inherits a root manifest. Some roots have a role-only agent64;
-- children inherit their parent's capture. No published version pins that agent.
INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id)
SELECT company_id,id,coalesce(parent_run_id,id),md5('manifest'||coalesce(parent_run_id,id)::text)::uuid
FROM workflow_runs;
INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,
    department_id,principal_id,agent_id,agent_revision_id)
SELECT run.company_id,run.id,md5('manifest'||run.id::text)::uuid,run.version_id,'start','lead',
    md5('department'||tenant.t)::uuid,md5('principal'||tenant.t)::uuid,
    md5('agent'||tenant.t||'-64')::uuid,md5('revision'||tenant.t)::uuid
FROM generate_series(1,100) AS tenant(t)
JOIN workflow_runs AS run ON run.company_id=md5('company'||tenant.t)::uuid
WHERE run.parent_run_id IS NULL AND run.channel_id IN (
    md5('channel'||tenant.t||'-0')::uuid,md5('channel'||tenant.t||'-4')::uuid,
    md5('channel'||tenant.t||'-10')::uuid,md5('channel'||tenant.t||'-14')::uuid,
    md5('channel'||tenant.t||'-20')::uuid,md5('channel'||tenant.t||'-24')::uuid,
    md5('channel'||tenant.t||'-30')::uuid);
ANALYZE workflow_versions;
ANALYZE workflow_runs;
ANALYZE run_dependency_manifests;
ANALYZE workflow_run_role_bindings;
\echo '== Table and index sizes (temporary clones)'
SELECT class.relname, class.reltuples::bigint AS rows, pg_size_pretty(pg_relation_size(class.oid)) AS size
  FROM pg_class AS class
 WHERE class.relpersistence = 't' AND class.relkind IN ('r', 'i')
 ORDER BY class.relkind DESC, class.relname;
\echo '== Shipped indexes on public.workflow_runs and public.workflow_versions'
SELECT indexdef FROM pg_indexes
 WHERE schemaname = 'public' AND tablename IN ('workflow_runs', 'workflow_versions') ORDER BY indexdef;
SET statement_timeout='15s';
SET plan_cache_mode=force_generic_plan;
PREPARE role_probe AS SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM ((SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM (SELECT version.id AS used_version FROM workflow_versions AS version WHERE version.company_id = $1 AND $2 = ANY(version.agent_ids)) AS agent_versions CROSS JOIN LATERAL (SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM workflow_runs WHERE company_id = $3 AND version_id = agent_versions.used_version ORDER BY created_at DESC, id DESC LIMIT $4) AS run ORDER BY created_at DESC, id DESC LIMIT $5) UNION (SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM workflow_runs WHERE company_id = $6 AND id IN (SELECT ownership.run_id FROM run_dependency_manifests AS ownership JOIN workflow_run_role_bindings AS role_binding ON role_binding.company_id=ownership.company_id AND role_binding.run_id=ownership.root_run_id AND role_binding.manifest_id=ownership.manifest_id WHERE ownership.company_id=$7 AND role_binding.agent_id=$8 AND role_binding.resolution_revision=0) ORDER BY created_at DESC, id DESC LIMIT $9)) AS matching_runs ORDER BY created_at DESC, id DESC LIMIT $10;
EXPLAIN EXECUTE role_probe(md5('company1')::uuid, md5('agent1-64')::uuid, md5('company1')::uuid, 201, 201, md5('company1')::uuid, md5('company1')::uuid, md5('agent1-64')::uuid, 201, 201);
CREATE INDEX role_agent_probe ON workflow_run_role_bindings(company_id,agent_id,run_id,manifest_id) WHERE resolution_revision=0;
CREATE INDEX role_owner_probe ON run_dependency_manifests(company_id,root_run_id,manifest_id,run_id);
ANALYZE workflow_run_role_bindings; ANALYZE run_dependency_manifests;
DEALLOCATE role_probe;
SET plan_cache_mode=force_generic_plan;
PREPARE role_probe AS SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM ((SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM (SELECT version.id AS used_version FROM workflow_versions AS version WHERE version.company_id = $1 AND $2 = ANY(version.agent_ids)) AS agent_versions CROSS JOIN LATERAL (SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM workflow_runs WHERE company_id = $3 AND version_id = agent_versions.used_version ORDER BY created_at DESC, id DESC LIMIT $4) AS run ORDER BY created_at DESC, id DESC LIMIT $5) UNION (SELECT id, binding_id, workflow_id, version_id, channel_id, thread_id, state, waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, finished_at FROM workflow_runs WHERE company_id = $6 AND id IN (SELECT ownership.run_id FROM run_dependency_manifests AS ownership JOIN workflow_run_role_bindings AS role_binding ON role_binding.company_id=ownership.company_id AND role_binding.run_id=ownership.root_run_id AND role_binding.manifest_id=ownership.manifest_id WHERE ownership.company_id=$7 AND role_binding.agent_id=$8 AND role_binding.resolution_revision=0) ORDER BY created_at DESC, id DESC LIMIT $9)) AS matching_runs ORDER BY created_at DESC, id DESC LIMIT $10;
EXPLAIN EXECUTE role_probe(md5('company1')::uuid, md5('agent1-64')::uuid, md5('company1')::uuid, 201, 201, md5('company1')::uuid, md5('company1')::uuid, md5('agent1-64')::uuid, 201, 201);
EXPLAIN (ANALYZE,BUFFERS) EXECUTE role_probe(md5('company1')::uuid, md5('agent1-64')::uuid, md5('company1')::uuid, 201, 201, md5('company1')::uuid, md5('company1')::uuid, md5('agent1-64')::uuid, 201, 201);
