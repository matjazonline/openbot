-- Reviewer B, FIXAPP-P05-G1-R1: does D1.1+D1.3 (notice deliveries carry company_id with
-- ON DELETE CASCADE; reviewer->notice FK composite NO ACTION) still let a company be deleted?
-- Fixture rows are inserted with triggers disabled (session_replication_role=replica); only
-- NOT NULL/CHECK apply. The DELETE runs with triggers enabled. Everything is rolled back.
\set ON_ERROR_STOP 0
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO workflow_runs(company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline,state,waiting_reason)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),1,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'\x7b7d','{}','{}','{}',10,1000,now()+interval '1 day','waiting','decision');
INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','start',1);
INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e3','decision',now()+interval '1 day');
INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e4','00000000-0000-0000-0000-0000000000e3','business',repeat('a',64));
INSERT INTO message_deliveries(id, external_destination, correlation_id, transport, purpose, idempotency_key, max_attempts)
  VALUES ('00000000-0000-0000-0000-0000000000d1','reviewer@example.test',gen_random_uuid(),'email','notification','notification:workflow-decision:x',3);
INSERT INTO workflow_decision_reviewers(company_id,user_id,decision_id,ordinal,principal_id,notice_delivery_id) VALUES ('00000000-0000-0000-0000-0000000000c1',gen_random_uuid(),'00000000-0000-0000-0000-0000000000e4',0,gen_random_uuid(),'00000000-0000-0000-0000-0000000000d1');
SET LOCAL session_replication_role = origin;

\echo '=== A. today (r0 schema): delete the company'
SAVEPOINT a;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SELECT 'A remaining deliveries', count(*) FROM message_deliveries WHERE id='00000000-0000-0000-0000-0000000000d1';
ROLLBACK TO SAVEPOINT a;

\echo '=== B. D1.1 + D1.3 as written (composite NO ACTION reviewer->notice FK)'
SAVEPOINT b;
ALTER TABLE message_deliveries DROP CONSTRAINT message_deliveries_attribution_check;
UPDATE message_deliveries SET company_id='00000000-0000-0000-0000-0000000000c1' WHERE id='00000000-0000-0000-0000-0000000000d1';
ALTER TABLE message_deliveries ADD CONSTRAINT message_deliveries_attribution_check CHECK (((channel_id IS NOT NULL) AND (message_id IS NOT NULL) AND (source_binding_id IS NOT NULL) AND (destination_binding_id IS NOT NULL)) OR ((channel_id IS NULL) AND (message_id IS NULL) AND (source_binding_id IS NULL) AND (destination_binding_id IS NULL) AND (depends_on_delivery_id IS NULL) AND (external_destination IS NOT NULL) AND (purpose = 'notification')));
ALTER TABLE workflow_decision_reviewers DROP CONSTRAINT workflow_decision_reviewers_notice_fkey;
ALTER TABLE workflow_decision_reviewers ADD CONSTRAINT workflow_decision_reviewers_notice_fkey FOREIGN KEY (company_id, notice_delivery_id) REFERENCES message_deliveries(company_id, id);
SAVEPOINT b_delete;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT b_delete;

\echo '=== C. same, reviewer->notice FK DEFERRABLE INITIALLY DEFERRED (checked at commit, emulated by SET CONSTRAINTS ... IMMEDIATE after the delete)'
ALTER TABLE workflow_decision_reviewers DROP CONSTRAINT workflow_decision_reviewers_notice_fkey;
ALTER TABLE workflow_decision_reviewers ADD CONSTRAINT workflow_decision_reviewers_notice_fkey FOREIGN KEY (company_id, notice_delivery_id) REFERENCES message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED;
SAVEPOINT c_delete;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SET CONSTRAINTS workflow_decision_reviewers_notice_fkey IMMEDIATE;
SELECT 'C remaining reviewers', count(*) FROM workflow_decision_reviewers WHERE decision_id='00000000-0000-0000-0000-0000000000e4';
ROLLBACK TO SAVEPOINT c_delete;

\echo '=== D. same, reviewer->notice FK ON DELETE CASCADE'
ALTER TABLE workflow_decision_reviewers DROP CONSTRAINT workflow_decision_reviewers_notice_fkey;
ALTER TABLE workflow_decision_reviewers ADD CONSTRAINT workflow_decision_reviewers_notice_fkey FOREIGN KEY (company_id, notice_delivery_id) REFERENCES message_deliveries(company_id, id) ON DELETE CASCADE;
SAVEPOINT d_delete;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SELECT 'D remaining reviewers', count(*) FROM workflow_decision_reviewers WHERE decision_id='00000000-0000-0000-0000-0000000000e4';
ROLLBACK TO SAVEPOINT d_delete;
ROLLBACK TO SAVEPOINT b;
ROLLBACK;
