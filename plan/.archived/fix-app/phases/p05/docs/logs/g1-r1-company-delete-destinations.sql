-- Reviewer B, FIXAPP-P05-G1-R1: company deletion with a workflow message destination, today and
-- with D4's FKs (thread_messages composite NO ACTION; destination_channel_id -> channels NO ACTION).
-- Rows inserted with triggers disabled; DELETE runs with triggers; all rolled back.
\set ON_ERROR_STOP 0
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO principals(id, company_id, kind, display_label) VALUES ('00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000c1','system','sys');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-0000000000c1','ch','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-0000000000c1','ch2','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO threads(id, company_id, channel_id, subject) VALUES ('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','s');
INSERT INTO messages(id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash) VALUES ('00000000-0000-0000-0000-000000000021','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','agent',gen_random_uuid(),decode(repeat('00',32),'hex'));
INSERT INTO thread_messages(id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','conversation');
INSERT INTO workflow_runs(company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline,channel_id)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),1,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'\x7b7d','{}','{}','{}',10,1000,now()+interval '1 day','00000000-0000-0000-0000-0000000000f1');
INSERT INTO workflow_message_destinations(company_id, run_id, invocation_id, ordinal, kind, destination_id, thread_id, message_id)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),0,'channel','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021');
SET LOCAL session_replication_role = origin;

\echo '=== A. today: delete the company'
SAVEPOINT a;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT a;

\echo '=== B. D4 FKs (thread_messages composite NO ACTION + destination_channel_id NO ACTION), old message/thread FKs dropped'
SAVEPOINT b;
ALTER TABLE workflow_message_destinations ADD COLUMN channel_id uuid, ADD COLUMN destination_channel_id uuid;
SET LOCAL session_replication_role = replica;
UPDATE workflow_message_destinations SET channel_id='00000000-0000-0000-0000-0000000000f2', destination_channel_id='00000000-0000-0000-0000-0000000000f2';
SET LOCAL session_replication_role = origin;
ALTER TABLE workflow_message_destinations DROP CONSTRAINT workflow_message_destination_message, DROP CONSTRAINT workflow_message_destination_thread;
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_thread_message FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id);
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_channel FOREIGN KEY (company_id, destination_channel_id) REFERENCES channels(company_id, id);
SAVEPOINT b_delete;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT b_delete;
\echo '=== B2. D4 with only the channel FK'
ALTER TABLE workflow_message_destinations DROP CONSTRAINT d4_thread_message;
SAVEPOINT b2;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT b2;
\echo '=== B3. D4 with only the thread_messages FK'
ALTER TABLE workflow_message_destinations DROP CONSTRAINT d4_channel;
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_thread_message FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id);
SAVEPOINT b3;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT b3;
ROLLBACK TO SAVEPOINT b3;
\echo '=== C. D4 FKs DEFERRABLE INITIALLY DEFERRED, plus destination_delivery and destination_message made deferred (emulated with SET CONSTRAINTS IMMEDIATE after the delete)'
ALTER TABLE workflow_message_destinations DROP CONSTRAINT d4_thread_message;
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_thread_message FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_channel FOREIGN KEY (company_id, destination_channel_id) REFERENCES channels(company_id, id) DEFERRABLE INITIALLY DEFERRED;
SAVEPOINT c;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT 'C remaining destinations', count(*) FROM workflow_message_destinations;
ROLLBACK TO SAVEPOINT c;
ROLLBACK TO SAVEPOINT b;
ROLLBACK;
