BEGIN;
CREATE TEMP TABLE background_tasks(company_id uuid,workflow_execution_id uuid,id uuid,status text,run_at timestamptz,lock_expires_at timestamptz,retry_count int,max_retries int);
CREATE TEMP TABLE workflow_executions(company_id uuid,run_id uuid,id uuid,completed_at timestamptz);
CREATE TEMP TABLE workflow_runs(company_id uuid,id uuid,state text,deadline timestamptz);
CREATE TEMP TABLE workflow_waits(company_id uuid,run_id uuid,execution_id uuid,id uuid,state text,reason text,deadline timestamptz,event_name text,correlation text,child_run_id uuid);
CREATE TEMP TABLE workflow_local_agent_calls(company_id uuid,run_id uuid,execution_id uuid,wait_id uuid,operation_id uuid,ready_at timestamptz);
CREATE TEMP TABLE local_agent_operations(company_id uuid,id uuid,command jsonb,outcome text,observed_turn_state text,deadline timestamptz,delivery_attempted_at timestamptz,dispatch jsonb);
INSERT INTO background_tasks VALUES('00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-000000000003','00000000-0000-4000-8000-000000000004','completed',now(),NULL,0,3);
INSERT INTO workflow_executions SELECT company_id,'00000000-0000-4000-8000-000000000002',workflow_execution_id,NULL FROM background_tasks;
INSERT INTO workflow_runs SELECT company_id,run_id,'waiting',now()+interval '1 hour' FROM workflow_executions;
INSERT INTO workflow_waits SELECT company_id,run_id,id,'00000000-0000-4000-8000-000000000005','waiting','effect',now()+interval '30 minutes',NULL,NULL,NULL FROM workflow_executions;
INSERT INTO workflow_local_agent_calls SELECT company_id,run_id,execution_id,id,'00000000-0000-4000-8000-000000000006',now() FROM workflow_waits;
INSERT INTO local_agent_operations SELECT company_id,operation_id,'{"kind":"create"}','succeeded','completed',now()+interval '30 minutes',now(),'{}' FROM workflow_local_agent_calls;
SELECT 'actual_worker_ready_candidates' AS check_name,count(*) FROM (
                SELECT job.company_id, execution.run_id, execution.id AS execution_id,
                       job.id AS job_id,
                       CASE WHEN execution.completed_at IS NULL AND run.state IN ('queued','running','waiting') AND run.deadline <= clock_timestamp() THEN 'deadline'
                            WHEN EXISTS (SELECT 1 FROM workflow_message_actions AS action
                WHERE action.company_id = execution.company_id AND action.execution_id = execution.id
                  AND action.state IN ('pending','uncertain') AND action.consume_after <= clock_timestamp()
                  AND EXISTS (SELECT 1 FROM workflow_message_delivery_events AS event
                      WHERE event.company_id = action.company_id AND event.invocation_id = action.invocation_id
                        AND event.seq > action.consumed_through)) THEN 'message_receipt'
                            WHEN wait.id IS NOT NULL THEN 'wait'
                            WHEN job.status = 'processing' THEN 'expired'
                            WHEN job.retry_count >= job.max_retries THEN 'exhausted' ELSE 'job' END AS work
                FROM background_tasks AS job
                JOIN workflow_executions AS execution
                  ON execution.company_id = job.company_id AND execution.id = job.workflow_execution_id
                JOIN workflow_runs AS run
                  ON run.company_id = execution.company_id AND run.id = execution.run_id
                LEFT JOIN workflow_waits AS wait
                  ON wait.company_id = execution.company_id AND wait.execution_id = execution.id
                 AND wait.run_id = run.id AND wait.state = 'waiting'
                WHERE ((execution.completed_at IS NULL
                  AND (
                    (run.state IN ('queued','running','waiting') AND run.deadline <= clock_timestamp())
                    OR (wait.id IS NULL AND run.state IN ('queued','running')
                     AND run.deadline > clock_timestamp()
                     AND ((job.status = 'pending' AND job.run_at <= clock_timestamp()
                           )
                          OR (job.status = 'processing' AND job.lock_expires_at <= clock_timestamp())))
                    OR (wait.id IS NOT NULL AND (
                        wait.deadline <= clock_timestamp() OR run.deadline <= clock_timestamp()
                        OR run.state IN ('cancelled','failed','succeeded')
                        -- Run-scoped events wake only event waits (05-D2 §11): a persisted,
                        -- unconsumed match, however early it arrived.
                        OR (wait.reason = 'event' AND EXISTS (SELECT 1 FROM workflow_wait_events AS event
                            WHERE event.company_id = wait.company_id AND event.run_id = wait.run_id
                              AND event.consumed_execution_id IS NULL
                              AND event.event_name = wait.event_name AND event.correlation = wait.correlation))
                        -- A child_run wait wakes on its child's terminal fact (07-D1 §D1.4).
                        OR (wait.reason = 'child_run' AND EXISTS (SELECT 1 FROM workflow_runs AS child
                            WHERE child.company_id = wait.company_id AND child.id = wait.child_run_id
                              AND child.terminal_wakeup_sequence IS NOT NULL))
                        -- An effect wait wakes on its action's receipt or on a finality breach
                        -- (04.11-D2 §4.4, D4 §M); otherwise only at the run deadline.
                        OR (wait.reason = 'effect' AND (
                            EXISTS (SELECT 1 FROM workflow_message_actions AS action
                                JOIN workflow_action_receipts AS receipt
                                  ON receipt.company_id = action.company_id AND receipt.invocation_id = action.invocation_id
                                WHERE action.company_id = wait.company_id AND action.execution_id = wait.execution_id
                                  AND action.wait_id = wait.id)
                            OR workflow_action_has_evidence_conflict(wait.company_id, wait.execution_id)))))
                  )) OR EXISTS (SELECT 1 FROM workflow_message_actions AS action
                WHERE action.company_id = execution.company_id AND action.execution_id = execution.id
                  AND action.state IN ('pending','uncertain') AND action.consume_after <= clock_timestamp()
                  AND EXISTS (SELECT 1 FROM workflow_message_delivery_events AS event
                      WHERE event.company_id = action.company_id AND event.invocation_id = action.invocation_id
                        AND event.seq > action.consumed_through)))
) AS candidates;
SELECT 'sweeper_ready_candidates' AS check_name,count(*) FROM workflow_waits AS wait WHERE wait.state='waiting' AND EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=wait.company_id AND call.wait_id=wait.id AND call.ready_at IS NOT NULL);
UPDATE workflow_waits SET deadline=now()-interval '1 second';
SELECT 'actual_worker_deadline_candidates' AS check_name,count(*) FROM (
                SELECT job.company_id, execution.run_id, execution.id AS execution_id,
                       job.id AS job_id,
                       CASE WHEN execution.completed_at IS NULL AND run.state IN ('queued','running','waiting') AND run.deadline <= clock_timestamp() THEN 'deadline'
                            WHEN EXISTS (SELECT 1 FROM workflow_message_actions AS action
                WHERE action.company_id = execution.company_id AND action.execution_id = execution.id
                  AND action.state IN ('pending','uncertain') AND action.consume_after <= clock_timestamp()
                  AND EXISTS (SELECT 1 FROM workflow_message_delivery_events AS event
                      WHERE event.company_id = action.company_id AND event.invocation_id = action.invocation_id
                        AND event.seq > action.consumed_through)) THEN 'message_receipt'
                            WHEN wait.id IS NOT NULL THEN 'wait'
                            WHEN job.status = 'processing' THEN 'expired'
                            WHEN job.retry_count >= job.max_retries THEN 'exhausted' ELSE 'job' END AS work
                FROM background_tasks AS job
                JOIN workflow_executions AS execution
                  ON execution.company_id = job.company_id AND execution.id = job.workflow_execution_id
                JOIN workflow_runs AS run
                  ON run.company_id = execution.company_id AND run.id = execution.run_id
                LEFT JOIN workflow_waits AS wait
                  ON wait.company_id = execution.company_id AND wait.execution_id = execution.id
                 AND wait.run_id = run.id AND wait.state = 'waiting'
                WHERE ((execution.completed_at IS NULL
                  AND (
                    (run.state IN ('queued','running','waiting') AND run.deadline <= clock_timestamp())
                    OR (wait.id IS NULL AND run.state IN ('queued','running')
                     AND run.deadline > clock_timestamp()
                     AND ((job.status = 'pending' AND job.run_at <= clock_timestamp()
                           )
                          OR (job.status = 'processing' AND job.lock_expires_at <= clock_timestamp())))
                    OR (wait.id IS NOT NULL AND (
                        wait.deadline <= clock_timestamp() OR run.deadline <= clock_timestamp()
                        OR run.state IN ('cancelled','failed','succeeded')
                        -- Run-scoped events wake only event waits (05-D2 §11): a persisted,
                        -- unconsumed match, however early it arrived.
                        OR (wait.reason = 'event' AND EXISTS (SELECT 1 FROM workflow_wait_events AS event
                            WHERE event.company_id = wait.company_id AND event.run_id = wait.run_id
                              AND event.consumed_execution_id IS NULL
                              AND event.event_name = wait.event_name AND event.correlation = wait.correlation))
                        -- A child_run wait wakes on its child's terminal fact (07-D1 §D1.4).
                        OR (wait.reason = 'child_run' AND EXISTS (SELECT 1 FROM workflow_runs AS child
                            WHERE child.company_id = wait.company_id AND child.id = wait.child_run_id
                              AND child.terminal_wakeup_sequence IS NOT NULL))
                        -- An effect wait wakes on its action's receipt or on a finality breach
                        -- (04.11-D2 §4.4, D4 §M); otherwise only at the run deadline.
                        OR (wait.reason = 'effect' AND (
                            EXISTS (SELECT 1 FROM workflow_message_actions AS action
                                JOIN workflow_action_receipts AS receipt
                                  ON receipt.company_id = action.company_id AND receipt.invocation_id = action.invocation_id
                                WHERE action.company_id = wait.company_id AND action.execution_id = wait.execution_id
                                  AND action.wait_id = wait.id)
                            OR workflow_action_has_evidence_conflict(wait.company_id, wait.execution_id)))))
                  )) OR EXISTS (SELECT 1 FROM workflow_message_actions AS action
                WHERE action.company_id = execution.company_id AND action.execution_id = execution.id
                  AND action.state IN ('pending','uncertain') AND action.consume_after <= clock_timestamp()
                  AND EXISTS (SELECT 1 FROM workflow_message_delivery_events AS event
                      WHERE event.company_id = action.company_id AND event.invocation_id = action.invocation_id
                        AND event.seq > action.consumed_through)))
) AS candidates;
UPDATE local_agent_operations SET outcome=NULL,observed_turn_state='running',deadline=now()-interval '1 second',dispatch='{"kind":"submission_intent"}';
UPDATE local_agent_operations SET outcome='expired' WHERE company_id='00000000-0000-4000-8000-000000000001' AND id='00000000-0000-4000-8000-000000000006' AND outcome IS NULL AND deadline<=clock_timestamp();
SELECT 'workflow_expiry_physical_capacity' AS check_name,outcome,observed_turn_state,(outcome IS NULL OR (outcome='uncertain' AND command->>'kind' IN ('create','send') AND COALESCE(observed_turn_state NOT IN ('completed','failed','stopped'),true))) AS counted FROM local_agent_operations;
ROLLBACK;
