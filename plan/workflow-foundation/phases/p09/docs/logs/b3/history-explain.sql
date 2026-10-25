BEGIN;
CREATE TEMP TABLE workflow_runs (id uuid PRIMARY KEY, company_id uuid, channel_id uuid, thread_id uuid);
CREATE TEMP TABLE workflow_executions (id uuid PRIMARY KEY, company_id uuid, step_id text);
CREATE TEMP TABLE workflow_waits (id uuid PRIMARY KEY, company_id uuid, state text);
CREATE TEMP TABLE workflow_decisions (id uuid PRIMARY KEY, company_id uuid, run_id uuid, execution_id uuid, wait_id uuid, created_at timestamptz, proposal_digest text, subject_kind text, choice text, feedback text);
CREATE INDEX workflow_decisions_history ON workflow_decisions(company_id,run_id,created_at DESC,id DESC);
INSERT INTO workflow_runs SELECT md5('run-'||ordinal)::uuid,md5('company')::uuid,NULL,NULL FROM generate_series(1,100) AS seed(ordinal);
INSERT INTO workflow_executions SELECT md5('execution-'||ordinal)::uuid,md5('company')::uuid,'review' FROM generate_series(1,50000) AS seed(ordinal);
INSERT INTO workflow_waits SELECT md5('wait-'||ordinal)::uuid,md5('company')::uuid,'completed' FROM generate_series(1,50000) AS seed(ordinal);
INSERT INTO workflow_decisions SELECT md5('decision-'||ordinal)::uuid,md5('company')::uuid,md5('run-'||(1+ordinal%100))::uuid,md5('execution-'||ordinal)::uuid,md5('wait-'||ordinal)::uuid,now()-ordinal*interval '1 second','digest','business','revise',repeat('feedback',100) FROM generate_series(1,50000) AS seed(ordinal);
ANALYZE workflow_runs; ANALYZE workflow_executions; ANALYZE workflow_waits; ANALYZE workflow_decisions;
PREPARE history(uuid,uuid[],uuid,uuid,bigint) AS

WITH eligible AS MATERIALIZED (
    SELECT run.id AS run_id FROM unnest($2::uuid[]) AS related(run_id)
    JOIN workflow_runs AS run ON run.company_id=$1 AND run.id=related.run_id
    WHERE run.channel_id IS NOT DISTINCT FROM $3::uuid AND run.thread_id IS NOT DISTINCT FROM $4::uuid
), heads AS MATERIALIZED (
    SELECT head.id,head.run_id,head.execution_id,head.wait_id,head.created_at FROM eligible
    CROSS JOIN LATERAL (
        SELECT id,run_id,execution_id,wait_id,created_at FROM workflow_decisions
        WHERE company_id=$1 AND run_id=eligible.run_id ORDER BY created_at DESC,id DESC LIMIT $5
    ) AS head
    ORDER BY head.created_at DESC,head.id DESC LIMIT $5
)
SELECT decision.id,decision.run_id,execution.step_id,decision.proposal_digest,decision.subject_kind,
       wait.state,decision.choice,decision.feedback,heads.created_at
FROM heads
JOIN workflow_decisions AS decision ON decision.company_id=$1 AND decision.id=heads.id
JOIN workflow_executions AS execution ON execution.company_id=$1 AND execution.id=heads.execution_id
JOIN workflow_waits AS wait ON wait.company_id=$1 AND wait.id=heads.wait_id
ORDER BY heads.created_at DESC,heads.id DESC
;
EXPLAIN (ANALYZE,BUFFERS) EXECUTE history(md5('company')::uuid,ARRAY[md5('run-1')::uuid,md5('run-2')::uuid,md5('run-3')::uuid,md5('run-4')::uuid,md5('run-5')::uuid,md5('run-6')::uuid,md5('run-7')::uuid,md5('run-8')::uuid,md5('run-9')::uuid,md5('run-10')::uuid,md5('run-11')::uuid,md5('run-12')::uuid,md5('run-13')::uuid,md5('run-14')::uuid,md5('run-15')::uuid,md5('run-16')::uuid,md5('run-17')::uuid,md5('run-18')::uuid,md5('run-19')::uuid,md5('run-20')::uuid,md5('run-21')::uuid,md5('run-22')::uuid],NULL,NULL,41);
ROLLBACK;
