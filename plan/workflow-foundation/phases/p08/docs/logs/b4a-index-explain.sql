-- p08 B4a: before/after plans for the B4a indexes, on temp probe tables that carry the columns,
-- keys and pre-existing indexes the production queries touch. Two companies of equal size, so a
-- company-prefix plan is not flattered by an empty neighbour. Nothing persists.
--   psql -h 127.0.0.1 -p 55480 -U workflow08 -d workflow08 -f b4a-index-explain.sql
\set ON_ERROR_STOP on
\pset pager off
SELECT setseed(0.42);

-- workflow_runs: 2 x 200k runs over 90 days; 50 channels and 20k threads per company. States:
-- 94% succeeded, 3% failed, 1% queued, 1% running, 1% waiting (half reconciliation).
CREATE TEMP TABLE runs_probe (
    company_id uuid NOT NULL, id uuid NOT NULL, binding_id uuid NOT NULL,
    channel_id uuid, thread_id uuid, state text NOT NULL, waiting_reason text,
    rerun_of_run_id uuid, created_at timestamptz NOT NULL,
    PRIMARY KEY (company_id, id)
);
INSERT INTO runs_probe
SELECT company.id, gen_random_uuid(), ('10000000-0000-0000-0000-' || lpad((n % 20)::text, 12, '0'))::uuid,
       CASE WHEN n % 10 = 0 THEN NULL ELSE ('20000000-0000-0000-0000-' || lpad((n % 50)::text, 12, '0'))::uuid END,
       CASE WHEN n % 10 IN (0, 1) THEN NULL ELSE ('30000000-0000-0000-0000-' || lpad((n % 20000)::text, 12, '0'))::uuid END,
       CASE WHEN r < 0.94 THEN 'succeeded' WHEN r < 0.97 THEN 'failed' WHEN r < 0.98 THEN 'queued'
            WHEN r < 0.99 THEN 'running' ELSE 'waiting' END,
       NULL, NULL, now() - (random() * interval '90 days')
FROM (VALUES ('00000000-0000-0000-0000-000000000001'::uuid), ('00000000-0000-0000-0000-000000000002'::uuid)) AS company(id)
CROSS JOIN LATERAL (SELECT n, random() AS r FROM generate_series(1, 200000) AS n) AS row;
UPDATE runs_probe SET waiting_reason = CASE WHEN random() < 0.5 THEN 'reconciliation' ELSE 'decision' END WHERE state = 'waiting';
-- A tenth of the failures were rerun.
INSERT INTO runs_probe
SELECT company_id, gen_random_uuid(), binding_id, channel_id, thread_id, 'succeeded', NULL, id, created_at + interval '1 minute'
FROM runs_probe WHERE state = 'failed' AND random() < 0.1;
CREATE INDEX ON runs_probe (company_id, created_at DESC, id DESC);
CREATE INDEX ON runs_probe (company_id, binding_id, created_at DESC, id DESC);

-- workflow_decisions: 2 x 50k, 1% open; reviewers: two each, 40 users per company.
CREATE TEMP TABLE decisions_probe (
    company_id uuid NOT NULL, id uuid NOT NULL, run_id uuid NOT NULL,
    submitted_at timestamptz, created_at timestamptz NOT NULL,
    PRIMARY KEY (company_id, id)
);
INSERT INTO decisions_probe
SELECT company_id, gen_random_uuid(), id, CASE WHEN random() < 0.99 THEN created_at + interval '1 hour' END, created_at
FROM runs_probe WHERE random() < 0.25 LIMIT 100000;
CREATE UNIQUE INDEX ON decisions_probe (id);
CREATE TEMP TABLE reviewers_probe (
    company_id uuid NOT NULL, decision_id uuid NOT NULL, principal_id uuid NOT NULL, user_id uuid NOT NULL,
    PRIMARY KEY (company_id, decision_id, principal_id)
);
INSERT INTO reviewers_probe
SELECT decision.company_id, decision.id, ('40000000-0000-0000-0000-' || lpad(reviewer.n::text, 12, '0'))::uuid,
       ('50000000-0000-0000-0000-' || lpad(reviewer.n::text, 12, '0'))::uuid
FROM decisions_probe AS decision
CROSS JOIN LATERAL (SELECT (abs(hashtext(decision.id::text)) + pick) % 40 AS n FROM generate_series(0, 1) AS pick) AS reviewer;

-- schedule_runs: 2 x 150k over 90 days, 0.5% failed materializations.
CREATE TEMP TABLE schedule_runs_probe (
    id uuid PRIMARY KEY, company_id uuid NOT NULL, schedule_id uuid NOT NULL, binding_id uuid NOT NULL,
    materialization_status text NOT NULL, created_at timestamptz NOT NULL
);
INSERT INTO schedule_runs_probe
SELECT gen_random_uuid(), company.id, ('60000000-0000-0000-0000-' || lpad((n % 100)::text, 12, '0'))::uuid,
       ('10000000-0000-0000-0000-' || lpad((n % 20)::text, 12, '0'))::uuid,
       CASE WHEN random() < 0.005 THEN 'failed' ELSE 'materialized' END, now() - (random() * interval '90 days')
FROM (VALUES ('00000000-0000-0000-0000-000000000001'::uuid), ('00000000-0000-0000-0000-000000000002'::uuid)) AS company(id)
CROSS JOIN generate_series(1, 150000) AS n;
CREATE INDEX ON schedule_runs_probe (schedule_id, created_at DESC, id DESC);

-- model_usage_calls: 2 x 400k calls, five per run on a fifth of the runs.
CREATE TEMP TABLE usage_probe (
    company_id uuid NOT NULL, request_id uuid NOT NULL, run_id uuid, provider text NOT NULL, model text NOT NULL,
    input_tokens bigint, output_tokens bigint, requested_at timestamptz NOT NULL,
    PRIMARY KEY (company_id, request_id)
);
INSERT INTO usage_probe
SELECT run.company_id, gen_random_uuid(), run.id, 'anthropic', 'claude', 100, 20, run.created_at
FROM (SELECT company_id, id, created_at FROM runs_probe WHERE random() < 0.2) AS run
CROSS JOIN generate_series(1, 5);
CREATE INDEX ON usage_probe (company_id, requested_at, request_id);

ANALYZE runs_probe; ANALYZE decisions_probe; ANALYZE reviewers_probe; ANALYZE schedule_runs_probe; ANALYZE usage_probe;
SELECT 'runs' AS probe, count(*) FROM runs_probe UNION ALL SELECT 'decisions', count(*) FROM decisions_probe
UNION ALL SELECT 'reviewers', count(*) FROM reviewers_probe UNION ALL SELECT 'schedule_runs', count(*) FROM schedule_runs_probe
UNION ALL SELECT 'usage', count(*) FROM usage_probe;

-- The statements, as the production SQL shapes them (company A, a reader of ten channels).
PREPARE failures AS
SELECT run.id FROM runs_probe AS run
WHERE run.company_id = '00000000-0000-0000-0000-000000000001' AND run.state = 'failed'
  AND run.created_at >= now() - interval '7 days'
  AND NOT EXISTS (SELECT 1 FROM runs_probe AS rerun WHERE rerun.company_id = run.company_id AND rerun.rerun_of_run_id = run.id);
PREPARE reconciliations AS
SELECT run.id FROM runs_probe AS run
WHERE run.company_id = '00000000-0000-0000-0000-000000000001' AND run.state = 'waiting' AND run.waiting_reason = 'reconciliation';
PREPARE counts AS
SELECT run.channel_id, run.state, count(*) FROM runs_probe AS run
WHERE run.company_id = '00000000-0000-0000-0000-000000000001' AND run.state IN ('queued', 'running', 'waiting')
  AND (run.channel_id = ANY (ARRAY(SELECT ('20000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid FROM generate_series(1, 10) AS n)) OR run.channel_id IS NULL)
GROUP BY run.channel_id, run.state;
PREPARE activity AS
SELECT DISTINCT ON (run.thread_id) run.thread_id, run.id, run.state, run.waiting_reason FROM runs_probe AS run
WHERE run.company_id = '00000000-0000-0000-0000-000000000001'
  AND run.thread_id = ANY (ARRAY(SELECT ('30000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid FROM generate_series(100, 149) AS n))
ORDER BY run.thread_id, run.state IN ('succeeded', 'failed', 'cancelled'), run.created_at DESC, run.id DESC;
PREPARE team_decisions AS
SELECT decision.id FROM decisions_probe AS decision
JOIN runs_probe AS run ON run.company_id = decision.company_id AND run.id = decision.run_id
WHERE decision.company_id = '00000000-0000-0000-0000-000000000001' AND decision.submitted_at IS NULL;
PREPARE my_decisions AS
SELECT decision.id FROM reviewers_probe AS reviewer
JOIN decisions_probe AS decision ON decision.company_id = reviewer.company_id AND decision.id = reviewer.decision_id
WHERE reviewer.company_id = '00000000-0000-0000-0000-000000000001'
  AND reviewer.user_id = '50000000-0000-0000-0000-000000000007' AND reviewer.principal_id = '40000000-0000-0000-0000-000000000007'
  AND decision.submitted_at IS NULL;
PREPARE schedule_failures AS
SELECT schedule_run.binding_id, schedule_run.created_at FROM schedule_runs_probe AS schedule_run
WHERE schedule_run.company_id = '00000000-0000-0000-0000-000000000001' AND schedule_run.materialization_status = 'failed'
  AND schedule_run.created_at >= now() - interval '7 days';
PREPARE run_usage AS
SELECT usage.run_id, usage.provider, usage.model, count(*), COALESCE(sum(usage.input_tokens), 0)::bigint
FROM usage_probe AS usage
WHERE usage.company_id = '00000000-0000-0000-0000-000000000001'
  AND usage.run_id = ANY (ARRAY(SELECT id FROM runs_probe WHERE company_id = '00000000-0000-0000-0000-000000000001' ORDER BY id LIMIT 20))
GROUP BY usage.run_id, usage.provider, usage.model;

\echo '==================== BEFORE (pre-existing indexes only) ===================='
\echo '== attention run_failure (workflow_runs_attention + workflow_runs_reruns)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE failures;
\echo '== attention run_reconciliation (workflow_runs_attention)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE reconciliations;
\echo '== run counts (workflow_runs_open_by_channel)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE counts;
\echo '== thread activity, 50 threads (workflow_runs_by_thread)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE activity;
\echo '== attention team-work decisions (workflow_decisions_open)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE team_decisions;
\echo '== attention my-work decisions (workflow_decision_reviewers_user)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE my_decisions;
\echo '== attention schedule refusals (schedule_runs_failed_recent)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE schedule_failures;
\echo '== simulation run usage, 20 runs (model_usage_calls_run)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE run_usage;

CREATE INDEX runs_probe_attention ON runs_probe (company_id, state, created_at DESC) WHERE state IN ('waiting', 'failed');
CREATE INDEX runs_probe_reruns ON runs_probe (company_id, rerun_of_run_id) WHERE rerun_of_run_id IS NOT NULL;
CREATE INDEX runs_probe_open_by_channel ON runs_probe (company_id, channel_id, state) WHERE state IN ('queued', 'running', 'waiting');
CREATE INDEX runs_probe_by_thread ON runs_probe (company_id, thread_id, created_at DESC) WHERE thread_id IS NOT NULL;
CREATE INDEX decisions_probe_open ON decisions_probe (company_id, run_id) WHERE submitted_at IS NULL;
CREATE INDEX reviewers_probe_user ON reviewers_probe (company_id, user_id);
CREATE INDEX schedule_runs_probe_failed_recent ON schedule_runs_probe (company_id, created_at DESC) WHERE materialization_status = 'failed';
CREATE INDEX usage_probe_run ON usage_probe (company_id, run_id) WHERE run_id IS NOT NULL;
ANALYZE runs_probe; ANALYZE decisions_probe; ANALYZE reviewers_probe; ANALYZE schedule_runs_probe; ANALYZE usage_probe;
SELECT relname AS index, pg_size_pretty(pg_relation_size(oid)) AS size FROM pg_class
WHERE relname IN ('runs_probe_attention', 'runs_probe_reruns', 'runs_probe_open_by_channel', 'runs_probe_by_thread',
                  'decisions_probe_open', 'reviewers_probe_user', 'schedule_runs_probe_failed_recent', 'usage_probe_run')
ORDER BY relname;

\echo '==================== AFTER (with the B4a indexes) ===================='
\echo '== attention run_failure (workflow_runs_attention + workflow_runs_reruns)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE failures;
\echo '== attention run_reconciliation (workflow_runs_attention)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE reconciliations;
\echo '== run counts (workflow_runs_open_by_channel)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE counts;
\echo '== thread activity, 50 threads (workflow_runs_by_thread)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE activity;
\echo '== attention team-work decisions (workflow_decisions_open)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE team_decisions;
\echo '== attention my-work decisions (workflow_decision_reviewers_user)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE my_decisions;
\echo '== attention schedule refusals (schedule_runs_failed_recent)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE schedule_failures;
\echo '== simulation run usage, 20 runs (model_usage_calls_run)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE run_usage;

\echo '==================== MY WORK without workflow_decision_reviewers_user ===================='
\echo '== attention my-work decisions, driven from workflow_decisions_open and the reviewer key'
DROP INDEX reviewers_probe_user;
ANALYZE reviewers_probe;
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, TIMING OFF, SUMMARY OFF) EXECUTE my_decisions;
