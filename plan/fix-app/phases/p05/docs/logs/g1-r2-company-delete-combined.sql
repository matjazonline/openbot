-- Reviewer B, FIXAPP-P05-G1-R2: one company holding every row the r2 FK choices touch on the
-- delete path (reviewer notice; channel, user+email, reply+email destinations; internal note),
-- with the r2 constraint shapes applied (D1.1/D1.3-r2, D4 + F3/F4/F7-r2). DELETE runs with
-- triggers; SET CONSTRAINTS ALL IMMEDIATE emulates COMMIT. Everything rolled back.
\set ON_ERROR_STOP 0
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO principals(id, company_id, kind, display_label) VALUES ('00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000c1','system','sys');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-0000000000c1','run','{"actor_type":"system","actor_id":null,"actor_name":"s"}'),('00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-0000000000c1','dest','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
-- threads: t1 channel destination thread (f2), t2 user email thread (f1), t3 reply thread (f1)
INSERT INTO threads(id, company_id, channel_id, subject) VALUES ('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','s'),('00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','s'),('00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','s');
-- messages: m1 channel post, m2 user email, m3 inbound source, m4 reply, m5 internal note
INSERT INTO messages(id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) VALUES
 ('00000000-0000-0000-0000-000000000021','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','system',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000022','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','system',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000023','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000024','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','system',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000025','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),'internal_only');
INSERT INTO thread_messages(id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','conversation'),
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000022','conversation'),
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000023','conversation'),
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000024','conversation'),
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000025','note');
INSERT INTO internal_notes(id, company_id, channel_id, thread_id, message_id, command_id, command_fingerprint, author_principal_id, provenance, message_audience)
  VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000025',gen_random_uuid(),'f','00000000-0000-0000-0000-0000000000b1','api','internal_only');
-- deliveries: d2 user email, d4 reply email, dn decision notice (standalone, company set per D1.1)
INSERT INTO message_deliveries(id, company_id, channel_id, message_id, source_binding_id, destination_binding_id, correlation_id, transport, purpose, idempotency_key, max_attempts, message_audience) VALUES
 ('00000000-0000-0000-0000-0000000000d2','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000022',gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'email','workflow_send','k2',3,'external_conversation'),
 ('00000000-0000-0000-0000-0000000000d4','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000024',gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'email','workflow_reply','k4',3,'external_conversation');
-- run chain for destinations and a parked decision
INSERT INTO workflow_runs(company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline,channel_id,state,waiting_reason)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),1,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'\x7b7d','{}','{}','{}',10,1000,now()+interval '1 day','00000000-0000-0000-0000-0000000000f1','waiting','decision');
INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','start',1);
INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e3','decision',now()+interval '1 day');
INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e4','00000000-0000-0000-0000-0000000000e3','business',repeat('a',64));
INSERT INTO message_deliveries(id, external_destination, correlation_id, transport, purpose, idempotency_key, max_attempts)
  VALUES ('00000000-0000-0000-0000-0000000000dd','reviewer@example.test',gen_random_uuid(),'email','notification','notification:workflow-decision:x',3);
INSERT INTO workflow_decision_reviewers(company_id,user_id,decision_id,ordinal,principal_id,notice_delivery_id) VALUES ('00000000-0000-0000-0000-0000000000c1',gen_random_uuid(),'00000000-0000-0000-0000-0000000000e4',0,'00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000dd');
INSERT INTO workflow_message_destinations(company_id, run_id, invocation_id, ordinal, kind, destination_id, thread_id, message_id, delivery_id, principal_id, recipient_address) VALUES
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000a9',0,'channel','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021',NULL,NULL,NULL),
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000a9',1,'user','00000000-0000-0000-0000-0000000000a1','00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000022','00000000-0000-0000-0000-0000000000d2','00000000-0000-0000-0000-0000000000b1','u@example.test'),
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000aa',0,'reply','00000000-0000-0000-0000-000000000023','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000024','00000000-0000-0000-0000-0000000000d4',NULL,'s@example.test');
-- r2 column additions, filled while triggers are off
ALTER TABLE workflow_message_destinations ADD COLUMN channel_id uuid, ADD COLUMN destination_channel_id uuid, ADD COLUMN destination_user_id uuid, ADD COLUMN replied_message_id uuid;
UPDATE workflow_message_destinations SET channel_id='00000000-0000-0000-0000-0000000000f2', destination_channel_id='00000000-0000-0000-0000-0000000000f2' WHERE kind='channel';
UPDATE workflow_message_destinations SET channel_id='00000000-0000-0000-0000-0000000000f1', destination_user_id='00000000-0000-0000-0000-0000000000a1' WHERE kind='user';
UPDATE workflow_message_destinations SET channel_id='00000000-0000-0000-0000-0000000000f1', replied_message_id='00000000-0000-0000-0000-000000000023' WHERE kind='reply';
ALTER TABLE message_deliveries DROP CONSTRAINT message_deliveries_attribution_check;
UPDATE message_deliveries SET company_id='00000000-0000-0000-0000-0000000000c1' WHERE id='00000000-0000-0000-0000-0000000000dd';
SET LOCAL session_replication_role = origin;

\echo '=== apply r2 constraint shapes'
-- D1.1 (old CHECK already dropped above to set the notice's company)
ALTER TABLE message_deliveries ALTER COLUMN company_id SET NOT NULL;
ALTER TABLE message_deliveries ADD CONSTRAINT message_deliveries_attribution_check CHECK (((channel_id IS NOT NULL) AND (message_id IS NOT NULL) AND (source_binding_id IS NOT NULL) AND (destination_binding_id IS NOT NULL)) OR ((channel_id IS NULL) AND (message_id IS NULL) AND (source_binding_id IS NULL) AND (destination_binding_id IS NULL) AND (depends_on_delivery_id IS NULL) AND (external_destination IS NOT NULL) AND (purpose = 'notification')));
-- D1.3 (F1 r2)
ALTER TABLE workflow_decision_reviewers DROP CONSTRAINT workflow_decision_reviewers_notice_fkey;
ALTER TABLE workflow_decision_reviewers ADD CONSTRAINT workflow_decision_reviewers_notice_fkey FOREIGN KEY (company_id, notice_delivery_id) REFERENCES message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED;
-- D4 + F3/F4 r2
ALTER TABLE workflow_message_destinations DROP CONSTRAINT workflow_message_destination_message, DROP CONSTRAINT workflow_message_destination_thread,
  DROP CONSTRAINT workflow_message_destination_delivery, DROP CONSTRAINT workflow_message_destination_principal;
ALTER TABLE workflow_message_destinations
  ADD CONSTRAINT r2_thread_message FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id),
  ADD CONSTRAINT r2_destination_channel FOREIGN KEY (company_id, destination_channel_id) REFERENCES channels(company_id, id) DEFERRABLE INITIALLY DEFERRED,
  ADD CONSTRAINT r2_destination_user FOREIGN KEY (destination_user_id) REFERENCES users(id),
  ADD CONSTRAINT r2_replied_message FOREIGN KEY (company_id, channel_id, thread_id, replied_message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id),
  ADD CONSTRAINT workflow_message_destination_delivery FOREIGN KEY (company_id, delivery_id, message_id) REFERENCES message_deliveries(company_id, id, message_id) DEFERRABLE INITIALLY DEFERRED,
  ADD CONSTRAINT workflow_message_destination_principal FOREIGN KEY (company_id, principal_id) REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;
-- D4 + F7 r2
ALTER TABLE internal_notes ADD CONSTRAINT internal_notes_thread_message_fk FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id) DEFERRABLE INITIALLY DEFERRED;

\echo '=== A. company deletion with every r2 shape (commit emulated)'
SAVEPOINT a;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT 'A left', (SELECT count(*) FROM message_deliveries) AS deliveries, (SELECT count(*) FROM workflow_message_destinations) AS destinations, (SELECT count(*) FROM workflow_decision_reviewers) AS reviewers, (SELECT count(*) FROM internal_notes) AS notes;
ROLLBACK TO SAVEPOINT a;

\echo '=== B. control: the reply FK stays immediate; message deletion is still refused at commit'
SAVEPOINT b;
DELETE FROM messages WHERE id='00000000-0000-0000-0000-000000000023';
SET CONSTRAINTS ALL IMMEDIATE;
ROLLBACK TO SAVEPOINT b;

\echo '=== C. control: the deferred reviewer FK still rejects a foreign-company notice at commit (guard trigger disabled to isolate the FK)'
SAVEPOINT c;
SET LOCAL session_replication_role = replica;
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c2','00000000-0000-0000-0000-0000000000a1','Co2','co-b2');
INSERT INTO message_deliveries(id, company_id, external_destination, correlation_id, transport, purpose, idempotency_key, max_attempts)
  VALUES ('00000000-0000-0000-0000-0000000000de','00000000-0000-0000-0000-0000000000c2','other@example.test',gen_random_uuid(),'email','notification','notification:workflow-decision:y',3);
SET LOCAL session_replication_role = origin;
ALTER TABLE workflow_decision_reviewers DISABLE TRIGGER workflow_decision_reviewer_guard;
INSERT INTO workflow_decision_reviewers(company_id,user_id,decision_id,ordinal,principal_id,notice_delivery_id) VALUES ('00000000-0000-0000-0000-0000000000c1',gen_random_uuid(),'00000000-0000-0000-0000-0000000000e4',1,gen_random_uuid(),'00000000-0000-0000-0000-0000000000de');
\echo 'insert accepted at statement time (deferred); now emulate commit:'
SET CONSTRAINTS ALL IMMEDIATE;
ROLLBACK TO SAVEPOINT c;
ROLLBACK;
